//! The side panel, top to bottom. Each section is a method so the order of
//! the panel reads off `show`.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy_egui::egui;
use nd_sim::{CitizenId, LandUse, Resource as Good};

use super::tools::warn_if;
use super::{help, hint, inspector, neon, saves, section, zone_color32, UiFlags, WARN};
use crate::assets::GameAssets;
use crate::camera::CameraMode;
use crate::render::{Lighting, Overlays, RenderStats};
use crate::sim::{Sim, SPEEDS};
use crate::tools::{EffectivePlan, Selection, ToolState};

/// Everything the panel reads or writes, borrowed for one pass.
pub struct PanelCtx<'a> {
    pub sim: &'a mut Sim,
    pub tools: &'a mut ToolState,
    pub selection: &'a mut Selection,
    pub plan: &'a mut EffectivePlan,
    pub saves: &'a mut saves::Saves,
    pub flags: &'a mut UiFlags,
    pub overlays: &'a mut Overlays,
    pub lighting: &'a mut Lighting,
    pub mode: &'a mut CameraMode,
    /// Where the favourites' "go" button asked the camera to fly; the
    /// caller applies it (a `Query` borrowed here would pin its lifetimes).
    pub fly_to: Option<Vec2>,
    pub assets: &'a GameAssets,
    pub stats: &'a RenderStats,
    pub diagnostics: &'a DiagnosticsStore,
}

impl PanelCtx<'_> {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.header(ui);
            self.clock_and_speed(ui);
            self.camera(ui);
            self.lighting(ui);
            self.project(ui);
            super::tools::tool_panel(ui, self.sim, self.tools, self.plan, self.assets);
            self.seed(ui);
            ui.separator();
            self.city(ui);
            self.power_and_treasury(ui);
            self.land_and_class(ui);
            self.market(ui);
            section(ui, "INSPECTOR");
            inspector::show(ui, self.sim, self.selection, self.assets);
            ui.separator();
            self.favourites(ui);
            saves::panel(ui, self.sim, self.tools, self.selection, self.plan, self.saves, self.flags);
            self.footer(ui);
        });
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.heading(egui::RichText::new("DISTRICT 2040").color(egui::Color32::from_rgb(235, 180, 90)).size(22.0));
        hint(ui, &format!("minds: {} · backend: {}", nd_sim::MindProvider::name(&self.sim.minds), self.sim.minds.backend_name()));
        ui.separator();
    }

    fn clock_and_speed(&mut self, ui: &mut egui::Ui) {
        let sim = &mut *self.sim;
        ui.horizontal(|ui| {
            neon(ui, &format!("Day {}  {}", sim.world.clock.day() + 1, sim.world.clock.format_hhmm()));
            ui.add_space(8.0);
            let label = if sim.paused { "▶ run [Space]" } else { "⏸ pause [Space]" };
            if ui.button(label).clicked() {
                sim.paused = !sim.paused;
            }
        });
        ui.horizontal(|ui| {
            for (i, (label, _)) in SPEEDS.iter().enumerate() {
                if ui.selectable_label(sim.speed_index == i, *label).clicked() {
                    sim.speed_index = i;
                }
            }
            hint(ui, "- / =");
        });
        ui.separator();
    }

    fn camera(&mut self, ui: &mut egui::Ui) {
        section(ui, "CAMERA");
        ui.horizontal(|ui| {
            let walking = *self.mode == CameraMode::Walk;
            if ui.selectable_label(!walking, "Orbit").clicked() {
                *self.mode = CameraMode::Orbit;
            }
            if ui.selectable_label(walking, "Walk the street [Tab]").clicked() {
                *self.mode = CameraMode::Walk;
            }
        });
        if *self.mode == CameraMode::Walk {
            hint(ui, help::CAMERA_WALK);
        } else {
            hint(ui, help::CAMERA_ORBIT);
            hint(ui, help::HOTKEYS);
            hint(ui, help::TOOL_KEYS);
        }
        ui.separator();
    }

    fn lighting(&mut self, ui: &mut egui::Ui) {
        section(ui, "LIGHTING");
        let lighting = &mut *self.lighting;
        ui.horizontal(|ui| {
            ui.checkbox(&mut lighting.follow_clock, "Follow the clock");
            if !lighting.follow_clock {
                ui.add(egui::Slider::new(&mut lighting.hour, 0.0..=24.0).text("hour").fixed_decimals(1));
            }
        });
        if !lighting.follow_clock {
            ui.horizontal(|ui| {
                for (label, h) in [("dawn", 6.5), ("noon", 12.5), ("dusk", 18.5), ("night", 23.0)] {
                    if ui.small_button(label).clicked() {
                        lighting.hour = h;
                    }
                }
            });
        }
        ui.separator();
    }

    fn project(&mut self, ui: &mut egui::Ui) {
        section(ui, "PROJECT");
        let sim = &mut *self.sim;
        let tools = &mut *self.tools;
        if let Some(p) = &sim.world.project {
            let steps = p.steps.len();
            let redo = p.redo.len();
            ui.label(format!("{steps} pending step{}{}", if steps == 1 { "" } else { "s" }, if redo > 0 { format!(", {redo} redoable") } else { String::new() }));
            ui.horizontal(|ui| {
                if ui.button("Implement  [Ctrl+Enter]").clicked() {
                    sim.world.implement();
                    tools.points.clear();
                    tools.status = "Project implemented".into();
                }
                if ui.button("Undo [Ctrl+Z]").clicked() {
                    sim.world.undo();
                }
                if ui.button("Redo [Ctrl+Y]").clicked() {
                    sim.world.redo();
                }
                if ui.button("Discard").clicked() {
                    sim.world.discard_project();
                    tools.points.clear();
                }
            });
        } else {
            hint(ui, help::PROJECT_NONE);
        }
        ui.separator();
    }

    /// The demo seed. The button is the deliberate path and works on any
    /// map; the hotkey (G, in `sim_hotkeys`) only on an empty one.
    fn seed(&mut self, ui: &mut egui::Ui) {
        if *self.mode == CameraMode::Walk {
            return;
        }
        let has_buildings = !self.sim.world.buildings.is_empty();
        let label = if has_buildings { "Seed demo district anyway" } else { "Seed demo district [G]" };
        let button = ui.button(label);
        if has_buildings {
            hint(ui, help::SEED_EXISTING);
        }
        if button.clicked() {
            self.sim.seed_demo_district();
            self.tools.status = "Demo district laid out around the centre".into();
        }
    }

    fn city(&mut self, ui: &mut egui::Ui) {
        let w = &self.sim.world;
        let e = &w.economy;
        section(ui, "CITY");
        let sites = w.buildings.values().filter(|b| b.construction.is_some()).count();
        ui.label(format!(
            "population {}   buildings {}   under construction {}   lots {} ({} vacant, {} unusable)",
            w.citizens.len(),
            w.buildings.len() - sites,
            sites,
            w.lots.len(),
            w.lots.values().filter(|l| l.building.is_none() && l.usable()).count(),
            w.lots.values().filter(|l| l.landlocked).count()
        ));
        if sites > 0 {
            ui.label(format!("building sites will add {} homes and {} jobs", e.homes_building, e.jobs_building));
        }
        ui.label(format!("homes {}/{}   jobs {}/{}   waiting to move in {}", e.homes_total - e.homes_free, e.homes_total, e.jobs_total - e.jobs_free, e.jobs_total, e.waiting_immigrants));
        ui.label(format!("parking {}/{} spaces in {} lots   circling today {}   fees today {:.0}", e.parking_used, e.parking_capacity, e.parking_lots, e.parking_street_today, e.parking_fees_today));
        ui.label(format!("walking trips today {}   walk-in sales {:.0}   pedestrian nodes {}", e.walk_trips_today, e.walk_in_sales_today, w.walk.nodes.len()));
        ui.label(format!("cars on the road {}   lanes {}   intersections {}", w.traffic.cars.len(), w.traffic.net.lanes.len(), w.traffic.net.intersections.iter().filter(|i| i.signalized).count()));
        ui.separator();
    }

    fn power_and_treasury(&mut self, ui: &mut egui::Ui) {
        let w = &self.sim.world;
        let e = &w.economy;
        let p = &e.power;
        section(ui, "POWER & TREASURY");
        ui.label(warn_if(p.served < 0.98, format!("supply {:.0} MW + import {:.0} MW   demand {:.0} MW   served {:.0}%", p.supply_mw, p.imported_mw, p.demand_mw, p.served * 100.0)));
        ui.label(format!("plants {}   buildings in the dark {}   brownout hours today {}   weather {} (cloud {:.0}%, wind {:.0}%)", p.plants, p.unpowered_buildings, p.brownout_hours_today, w.weather.label(), w.weather.cloud * 100.0, w.weather.wind * 100.0));
        ui.label(format!("treasury {:.0}   taxes so far today {:.0}   plant upkeep {:.0}/day   imported power {:.0} today", e.treasury, e.tax_today, p.upkeep_today, p.import_cost_today));
        ui.separator();
    }

    fn land_and_class(&mut self, ui: &mut egui::Ui) {
        section(ui, "LAND & CLASS");
        ui.checkbox(&mut self.overlays.land_value, "Land value overlay [V]");
        ui.checkbox(&mut self.overlays.paths, "Pedestrian network overlay [P]");
        ui.checkbox(&mut self.overlays.zoning, "Zoning outlines [O]");
        let e = &self.sim.world.economy;
        ui.label(format!("land value avg {:.2}   gini {:.2}   displaced {}   left town {}   priced out {}", e.avg_land_value, e.gini, e.displaced_total, e.emigrated_total, e.priced_out_total));
        ui.label(format!("housing  low {} · middle {} · high {}     rents {:.0} / {:.0} / {:.0}", e.tier_counts[0], e.tier_counts[1], e.tier_counts[2], e.avg_rent[0], e.avg_rent[1], e.avg_rent[2]));
        ui.label(format!("people   low {} · middle {} · high {}", e.class_counts[0], e.class_counts[1], e.class_counts[2]));
        ui.separator();
    }

    fn market(&mut self, ui: &mut egui::Ui) {
        section(ui, "MARKET");
        let e = &self.sim.world.economy;
        let m = &e.market;
        ui.label(format!("businesses {}   deals today {}   imports {:.0}   exports {:.0}   meals sold {}   hungry {}   closed {}", m.businesses, m.deals_today, m.imports_today, m.exports_today, m.meals_sold_today, m.hungry, m.closed_total));
        ui.horizontal_wrapped(|ui| {
            for r in [Good::Produce, Good::Grain, Good::Flour, Good::Meat, Good::Dairy, Good::Groceries, Good::BakedGoods, Good::Parts, Good::Electronics, Good::Goods] {
                let price = m.avg_price.get(&r).copied().unwrap_or(r.base_price());
                let stock = m.stock.get(&r).copied().unwrap_or(0.0);
                let hot = price > r.base_price() * 1.25;
                ui.label(warn_if(hot, format!("{} {:.1} ({:.0})", r.label(), price, stock)).small());
            }
        });
        hint(ui, "price (stock in town); red = scarce");
        hint(ui, "demand");
        for lu in LandUse::ALL {
            let col = zone_color32(self.assets.palette.zone_color(lu));
            ui.add(egui::ProgressBar::new(e.demand_for(lu)).text(lu.label()).fill(col.gamma_multiply(0.7)).desired_height(12.0));
        }
        ui.separator();
    }

    /// Starred people, with a button to select and one to fly the camera to
    /// them. Anyone who left town drops off the list.
    fn favourites(&mut self, ui: &mut egui::Ui) {
        let w = &self.sim.world;
        self.tools.favorites.retain(|id| w.citizens.contains_key(*id));
        if self.tools.favorites.is_empty() {
            return;
        }
        section(ui, "FAVOURITES");
        let mut pick: Option<CitizenId> = None;
        let mut jump: Option<nd_sim::Vec2> = None;
        let mut drop: Option<CitizenId> = None;
        for &id in &self.tools.favorites {
            let Some(c) = w.citizens.get(id) else { continue };
            ui.horizontal(|ui| {
                let sel = *self.selection == Selection::Citizen(id);
                if ui.selectable_label(sel, &c.name).clicked() {
                    pick = Some(id);
                }
                hint(ui, c.activity.label());
                if ui.small_button("go").on_hover_text("Select and fly the camera to them").clicked() {
                    pick = Some(id);
                    jump = Some(c.pos);
                }
                if ui.small_button("★").on_hover_text("Remove from favourites").clicked() {
                    drop = Some(id);
                }
            });
        }
        if let Some(id) = pick {
            *self.selection = Selection::Citizen(id);
        }
        if let Some(id) = drop {
            self.tools.favorites.retain(|f| *f != id);
        }
        if let Some(p) = jump {
            self.fly_to = Some(Vec2::new(p.x, p.y));
        }
        ui.separator();
    }

    /// Status line, asset errors and the HUD figures.
    fn footer(&mut self, ui: &mut egui::Ui) {
        if !self.flags.message.is_empty() {
            ui.label(egui::RichText::new(&self.flags.message).small());
        }
        if let Some(err) = &self.assets.last_error {
            ui.label(egui::RichText::new(err).small().color(egui::Color32::from_rgb(255, 60, 90)));
        }
        hint(ui, help::ASSETS_RELOAD);
        let sim = &*self.sim;
        let stats = self.stats;
        let fps = self.diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
        ui.horizontal_wrapped(|ui| {
            hint(ui, &format!("{fps:.0} fps  ·  {} ticks/frame  ·", sim.ticks_last_frame));
            // Effective speed: the share of the chosen speed the sim is
            // actually delivering. Below 0.9 the tick budget is throttling it.
            let eff = sim.effective_speed;
            let throttled = !sim.paused && eff < 0.9;
            let text = if sim.paused { "paused".to_string() } else { format!("running at {:.0}% of {}", eff * 100.0, SPEEDS[sim.speed_index].0) };
            let t = egui::RichText::new(text).small();
            ui.label(if throttled { t.color(WARN) } else { t.weak() });
            hint(ui, &format!("·  {} building meshes  ·  {}k triangles", stats.building_entities, (stats.street_triangles + stats.building_triangles) / 1000));
        });
    }
}
