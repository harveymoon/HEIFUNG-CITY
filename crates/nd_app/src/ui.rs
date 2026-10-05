//! egui side panel: clock, speed, project, tools, stats, inspector, save/load.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use nd_minds::Personality;
use nd_sim::LandUse;

use crate::assets::GameAssets;
use crate::render::Overlays;
use crate::sim::{Sim, SPEEDS};
use crate::tools::{Selection, Tool, ToolState};

const SAVE_PATH: &str = "saves/city.ron";

#[derive(Resource, Default)]
pub struct UiFlags {
    pub pointer_over_ui: bool,
    pub message: String,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiFlags>().add_systems(EguiPrimaryContextPass, side_panel);
    }
}

fn neon(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(egui::Color32::from_rgb(230, 225, 210)).strong());
}

fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).small().strong().color(egui::Color32::from_rgb(235, 180, 90)));
}

fn zone_color32(c: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb((c[0].min(1.0) * 255.0) as u8, (c[1].min(1.0) * 255.0) as u8, (c[2].min(1.0) * 255.0) as u8)
}

fn side_panel(
    mut contexts: EguiContexts,
    mut sim: ResMut<Sim>,
    mut tools: ResMut<ToolState>,
    mut selection: ResMut<Selection>,
    mut flags: ResMut<UiFlags>,
    mut overlays: ResMut<Overlays>,
    assets: Res<GameAssets>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    style(ctx);
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.viewport_rect()),
    );

    egui::Panel::left("neon_panel").default_size(340.0).show(&mut root, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(4.0);
            ui.heading(egui::RichText::new("DISTRICT 2040").color(egui::Color32::from_rgb(235, 180, 90)).size(22.0));
            ui.label(egui::RichText::new(format!("minds: {} · backend: {}", nd_sim::MindProvider::name(&sim.minds), sim.minds.backend_name())).small().weak());
            ui.separator();

            // Clock and speed.
            ui.horizontal(|ui| {
                neon(ui, &format!("Day {}  {}", sim.world.clock.day() + 1, sim.world.clock.format_hhmm()));
                ui.add_space(8.0);
                let label = if sim.paused { "▶ run" } else { "⏸ pause" };
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
            });
            ui.separator();

            // Project.
            section(ui, "PROJECT");
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
                    if ui.button("Undo").clicked() {
                        sim.world.undo();
                    }
                    if ui.button("Redo").clicked() {
                        sim.world.redo();
                    }
                    if ui.button("Discard").clicked() {
                        sim.world.discard_project();
                        tools.points.clear();
                    }
                });
            } else {
                ui.label(egui::RichText::new("No pending project. Drawing or bulldozing starts one.").weak().small());
            }
            ui.separator();

            // Tools.
            section(ui, "TOOLS");
            ui.horizontal_wrapped(|ui| {
                for (tool, label) in [
                    (Tool::Inspect, "Inspect [1]"),
                    (Tool::Road, "Road [2]"),
                    (Tool::Zone, "Zone [3]"),
                    (Tool::Edit, "Edit [4]"),
                    (Tool::Bulldoze, "Bulldoze [5]"),
                ] {
                    if ui.selectable_label(tools.tool == tool, label).clicked() {
                        tools.tool = tool;
                        tools.points.clear();
                        tools.drag = None;
                    }
                }
            });
            match tools.tool {
                Tool::Road => {
                    ui.add(egui::Slider::new(&mut tools.lanes_fwd, 0..=3).text("lanes forward"));
                    ui.add(egui::Slider::new(&mut tools.lanes_bwd, 0..=3).text("lanes backward"));
                    if tools.lanes_fwd + tools.lanes_bwd == 0 {
                        tools.lanes_fwd = 1;
                    }
                    ui.label(egui::RichText::new("Click to place nodes (snaps to existing nodes), Enter or right-click to add to the project, Backspace undoes a node.").small().weak());
                }
                Tool::Zone => {
                    ui.horizontal_wrapped(|ui| {
                        for lu in LandUse::ALL {
                            let text = egui::RichText::new(lu.label()).color(zone_color32(assets.palette.zone_color(lu)));
                            if ui.selectable_label(tools.land_use == lu, text).clicked() {
                                tools.land_use = lu;
                            }
                        }
                    });
                    ui.label(egui::RichText::new("Click polygon corners around a road, Enter or right-click to add to the project. Ctrl-click inside an existing zone to repaint it with the selected land use. Zones drawn over older ones take the ground.").small().weak());
                }
                Tool::Edit => {
                    ui.label(egui::RichText::new("Drag a control point to move it. Shift-click a road or zone edge to insert a point. Delete removes the hovered point.").small().weak());
                }
                Tool::Bulldoze => {
                    ui.label(egui::RichText::new("Click a road or zone to mark it for removal.").small().weak());
                }
                Tool::Inspect => {
                    ui.label(egui::RichText::new("Click a glowing citizen, a car, or a building.").small().weak());
                }
            }
            if !tools.status.is_empty() {
                ui.label(egui::RichText::new(&tools.status).small());
            }
            if ui.button("Seed demo district [G]").clicked() {
                sim.seed_demo_district();
                tools.status = "Demo district laid out around the centre".to_string();
            }
            ui.separator();

            // Stats.
            let e = &sim.world.economy;
            section(ui, "CITY");
            ui.label(format!(
                "population {}   buildings {}   lots {} ({} vacant)",
                sim.world.citizens.len(),
                sim.world.buildings.len(),
                sim.world.lots.len(),
                sim.world.lots.values().filter(|l| l.building.is_none()).count()
            ));
            ui.label(format!(
                "homes {}/{}   jobs {}/{}   waiting to move in {}",
                e.homes_total - e.homes_free,
                e.homes_total,
                e.jobs_total - e.jobs_free,
                e.jobs_total,
                e.waiting_immigrants
            ));
            ui.label(format!(
                "cars on the road {}   lanes {}   intersections {}",
                sim.world.traffic.cars.len(),
                sim.world.traffic.net.lanes.len(),
                sim.world.traffic.net.intersections.iter().filter(|i| i.signalized).count()
            ));
            ui.separator();
            section(ui, "LAND & CLASS");
            ui.checkbox(&mut overlays.land_value, "Land value overlay [V]");
            ui.label(format!(
                "land value avg {:.2}   gini {:.2}   displaced {}   left town {}   priced out {}",
                e.avg_land_value, e.gini, e.displaced_total, e.emigrated_total, e.priced_out_total
            ));
            ui.label(format!(
                "housing  low {} · middle {} · high {}     rents {:.0} / {:.0} / {:.0}",
                e.tier_counts[0], e.tier_counts[1], e.tier_counts[2], e.avg_rent[0], e.avg_rent[1], e.avg_rent[2]
            ));
            ui.label(format!("people   low {} · middle {} · high {}", e.class_counts[0], e.class_counts[1], e.class_counts[2]));
            ui.label(egui::RichText::new("demand").small().weak());
            for lu in LandUse::ALL {
                let col = zone_color32(assets.palette.zone_color(lu));
                ui.add(egui::ProgressBar::new(e.demand_for(lu)).text(lu.label()).fill(col.gamma_multiply(0.7)).desired_height(12.0));
            }
            ui.separator();

            // Inspector.
            section(ui, "INSPECTOR");
            match *selection {
                Selection::None => {
                    ui.label(egui::RichText::new("Nothing selected.").weak());
                }
                Selection::Citizen(id) => {
                    let Some(c) = sim.world.citizens.get(id) else {
                        *selection = Selection::None;
                        return;
                    };
                    let p = Personality::from_seed(c.seed);
                    neon(ui, &c.name);
                    ui.label(egui::RichText::new(p.summary()).small());
                    let home = c.home.and_then(|h| sim.world.buildings.get(h)).map(|b| b.name.as_str()).unwrap_or("none");
                    let work = c.work.and_then(|h| sim.world.buildings.get(h)).map(|b| b.name.as_str()).unwrap_or("unemployed");
                    ui.label(format!("{}  ·  home: {home}  ·  work: {work}", c.activity.label()));
                    ui.label(format!("mood {:.0}%  ·  arrived day {}", c.mood * 100.0, c.arrived_tick / 1440 + 1));
                    let rent = c.home.and_then(|h| sim.world.buildings.get(h)).map(|b| b.rent).unwrap_or(0.0);
                    ui.label(format!("{} class  ·  income {:.0}/day  ·  rent {:.0}/day  ·  savings {:.0}{}", c.class().label(), c.income, rent, c.savings, if c.rent_stress_days > 0 { format!("  ·  rent stress {}d", c.rent_stress_days) } else { String::new() }));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(format!("“{}”", c.thought)).italics().color(egui::Color32::from_rgb(220, 190, 140)));
                    ui.add_space(4.0);
                    let mut spotlight = c.spotlight;
                    if ui.checkbox(&mut spotlight, "Spotlight: route this citizen through the LLM mind").changed() {
                        if let Some(c) = sim.world.citizens.get_mut(id) {
                            c.spotlight = spotlight;
                        }
                    }
                    ui.label(egui::RichText::new("memories").small().weak());
                    if let Some(c) = sim.world.citizens.get(id) {
                        for m in c.memories.iter().rev().take(8) {
                            ui.label(egui::RichText::new(format!("d{} {:02}:{:02}  {}", m.tick / 1440 + 1, (m.tick % 1440) / 60, m.tick % 60, m.text)).small());
                        }
                    }
                }
                Selection::Building(id) => {
                    let Some(b) = sim.world.buildings.get(id) else {
                        *selection = Selection::None;
                        return;
                    };
                    neon(ui, &b.name);
                    ui.label(format!("{}  ·  {} tier  ·  {} floors  ·  built day {}", b.land_use.label(), b.tier.label(), b.floors, b.built_tick / 1440 + 1));
                    ui.label(format!("land value {:.2}  ·  rent {:.0}/day  ·  wage {:.0}/day  ·  condition {:.0}%{}", b.land_value, b.rent, b.wage(), b.condition * 100.0, if b.is_derelict() { "  ·  DERELICT" } else { "" }));
                    ui.label(format!("residents {}/{}  ·  workers {}/{}  ·  visitors {}", b.residents.len(), b.homes, b.workers.len(), b.jobs, b.visitors));
                    ui.label(egui::RichText::new("people here").small().weak());
                    let names: Vec<String> = b
                        .residents
                        .iter()
                        .chain(b.workers.iter())
                        .filter_map(|&cid| sim.world.citizens.get(cid).map(|c| c.name.clone()))
                        .take(10)
                        .collect();
                    for n in names {
                        ui.label(egui::RichText::new(n).small());
                    }
                }
            }
            ui.separator();

            // Save / load / assets.
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    flags.message = match sim.save(SAVE_PATH) {
                        Ok(()) => format!("saved {SAVE_PATH}"),
                        Err(e) => format!("save failed: {e}"),
                    };
                }
                if ui.button("Load").clicked() {
                    flags.message = match sim.load(SAVE_PATH) {
                        Ok(()) => {
                            *selection = Selection::None;
                            format!("loaded {SAVE_PATH}")
                        }
                        Err(e) => format!("load failed: {e}"),
                    };
                }
            });
            if !flags.message.is_empty() {
                ui.label(egui::RichText::new(&flags.message).small());
            }
            if let Some(err) = &assets.last_error {
                ui.label(egui::RichText::new(err).small().color(egui::Color32::from_rgb(255, 60, 90)));
            }
            ui.label(egui::RichText::new("assets/palette.ron and assets/rules/buildings.ron hot-reload while running.").small().weak());
            ui.label(egui::RichText::new("Camera: right-drag rotate · middle-drag or shift-drag pan · wheel or [ ] zoom · WASD/QE · F7/F8/F9 jump to a low/middle/high tier building").small().weak());
            ui.label(egui::RichText::new(format!("{} ticks/frame", sim.ticks_last_frame)).small().weak());
        });
    });

    flags.pointer_over_ui = ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
    Ok(())
}

fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgba_unmultiplied(8, 10, 20, 235);
    visuals.window_fill = egui::Color32::from_rgba_unmultiplied(8, 10, 20, 235);
    visuals.selection.bg_fill = egui::Color32::from_rgb(120, 85, 30);
    visuals.hyperlink_color = egui::Color32::from_rgb(235, 180, 90);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(150, 105, 40);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(70, 55, 35);
    ctx.set_visuals(visuals);
}
