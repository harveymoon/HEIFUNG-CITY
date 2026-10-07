//! The map legend: a small panel at the bottom right of the map with the
//! colour key of every data layer that is on, drawn from the same ramps the
//! renderer uses (`nd_procgen::overlay`), so the two cannot drift apart.

use bevy_egui::egui::{self, Color32};
use nd_procgen::overlay::{crime_color, heat_color, industry_hue, land_color, land_hue, pollution_color, traffic_color, VehicleClass};
use nd_sim::survey::LandLayer;
use nd_sim::IndustryKind;

use super::{hint, section};
use crate::assets::GameAssets;
use crate::render::Overlays;

fn linear(c: [f32; 3]) -> Color32 {
    Color32::from(egui::Rgba::from_rgb(c[0].min(1.0), c[1].min(1.0), c[2].min(1.0)))
}

/// A horizontal colour ramp with its end labels.
fn ramp(ui: &mut egui::Ui, color: impl Fn(f32) -> Color32, lo: &str, hi: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(180.0, 10.0), egui::Sense::hover());
    let steps = 36;
    for i in 0..steps {
        let t = (i as f32 + 0.5) / steps as f32;
        let x0 = rect.left() + rect.width() * i as f32 / steps as f32;
        let x1 = rect.left() + rect.width() * (i + 1) as f32 / steps as f32 + 0.5;
        ui.painter().rect_filled(egui::Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom())), 0.0, color(t));
    }
    ui.horizontal(|ui| {
        ui.set_width(180.0);
        hint(ui, lo);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| hint(ui, hi));
    });
}

pub fn show(ctx: &egui::Context, o: &Overlays, assets: &GameAssets, area: egui::Rect, here: Option<&str>) {
    if !(o.land_value || o.traffic || o.traffic_types || o.people || o.pollution || o.crime || o.map || o.land.is_some()) {
        return;
    }
    let pal = &assets.palette;
    egui::Area::new(egui::Id::new("map_legend"))
        .order(egui::Order::Middle)
        .interactable(false)
        .fixed_pos(egui::pos2(area.right() - 216.0, area.bottom() - 12.0))
        .pivot(egui::Align2::LEFT_BOTTOM)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(Color32::from_rgba_unmultiplied(8, 10, 20, 225)).show(ui, |ui| {
                ui.set_width(186.0);
                if o.land_value {
                    section(ui, "LAND VALUE");
                    let (a, b) = (pal.overlay_low, pal.overlay_high);
                    ramp(ui, |t| linear([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]), "low", "high");
                }
                if o.traffic {
                    section(ui, "TRAFFIC LOAD");
                    ramp(ui, |t| linear(traffic_color(t)), "free flow", "jammed");
                    hint(ui, "per lane: how full and how slow");
                    swatch(ui, crate::render::TRAFFIC_BLUE, "every vehicle; the city dimmed");
                    hint(ui, "outlined through the ground: cars in tunnels");
                }
                if o.traffic_types {
                    section(ui, "TRAFFIC TYPES");
                    ui.horizontal_wrapped(|ui| {
                        for class in VehicleClass::ALL {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                            ui.painter().circle_filled(r.center(), 5.0, linear(class.color()));
                            hint(ui, class.label());
                        }
                    });
                }
                if o.people {
                    section(ui, "PEOPLE NOW");
                    ramp(ui, |t| { let c = heat_color(t); linear([c[0], c[1], c[2]]) }, "few", "crowded");
                    hint(ui, "everyone where they are, indoors at their building");
                }
                if o.pollution {
                    section(ui, "POLLUTION AND NOISE");
                    ramp(ui, |t| { let c = pollution_color(t); linear([c[0], c[1], c[2]]) }, "clean", "as by a coal plant");
                }
                if o.crime {
                    section(ui, "CRIME");
                    ramp(ui, |t| { let c = crime_color(t); linear([c[0], c[1], c[2]]) }, "quiet", "hot spot");
                    hint(ui, "offences lately, fading a quarter a day");
                    swatch(ui, crate::render::POLICE_GLOW, "police car");
                    swatch(ui, [0.3, 0.9, 1.3], "patrol route");
                    swatch(ui, [2.2, 0.25, 0.3], "on a call, lights on");
                    swatch(ui, [1.6, 0.4, 0.1], "crime not yet answered");
                }
                if o.map {
                    section(ui, "MAP VIEW");
                    ui.horizontal_wrapped(|ui| {
                        for (label, c) in crate::render::MAP_ROAD_KEY {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 6.0), egui::Sense::hover());
                            ui.painter().rect_filled(r, 1.0, linear(c));
                            hint(ui, label);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for lu in [nd_sim::LandUse::Residential, nd_sim::LandUse::Commercial, nd_sim::LandUse::Industrial, nd_sim::LandUse::Entertainment, nd_sim::LandUse::Civic, nd_sim::LandUse::Park] {
                            let c = crate::render::map_zone_tint(lu);
                            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                            // The tints are dark on the map; shown brighter here so they read.
                            ui.painter().rect_filled(r, 2.0, linear([c[0] * 4.0, c[1] * 4.0, c[2] * 4.0]));
                            hint(ui, lu.label());
                            ui.add_space(4.0);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for (glyph, c, label) in crate::render::MAP_PIN_KEY {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                            ui.painter().circle_filled(r.center(), 6.5, Color32::from_rgb(c[0], c[1], c[2]));
                            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(9.0), Color32::from_rgb(12, 14, 22));
                            hint(ui, label);
                        }
                    });
                    hint(ui, "street names along the roads; tunnels dashed");
                }
                if let Some(l) = o.land {
                    land_legend(ui, l);
                    if let Some(t) = here {
                        ui.label(egui::RichText::new(t).small().color(Color32::from_rgb(230, 235, 255)));
                    }
                }
            });
        });
}

/// A colour swatch with its meaning.
fn swatch(ui: &mut egui::Ui, c: [f32; 3], label: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 8.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 2.0, linear(c));
        hint(ui, label);
    });
}

/// The key of the land survey layer: a ramp in its hue, or for "best
/// industry" a swatch per kind; then what the layer means and who wants it.
fn land_legend(ui: &mut egui::Ui, l: LandLayer) {
    section(ui, &l.label().to_uppercase());
    match l {
        LandLayer::BestIndustry => {
            ui.horizontal_wrapped(|ui| {
                for k in IndustryKind::ALL {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(r, 2.0, linear(industry_hue(k)));
                    hint(ui, k.key());
                }
            });
            hint(ui, "fainter where the lead is narrow");
        }
        LandLayer::Industry(_) => {
            let h = land_hue(l);
            ramp(ui, |t| { let c = land_color(h, t); linear([c[0], c[1], c[2]]) }, "unlikely", "likely");
        }
        _ => {
            let h = land_hue(l);
            ramp(ui, |t| { let c = land_color(h, t); linear([c[0], c[1], c[2]]) }, "none", "rich");
        }
    }
    hint(ui, l.hint());
    hint(ui, "K: next land layer");
}

/// The land layer's reading at the pointer, for the legend: the resource's
/// strength, an industry's chance, or the favourite kind.
pub fn land_here(w: &nd_sim::World, l: LandLayer, p: nd_sim::Vec2) -> String {
    if w.landscape.is_water(p) {
        return "under the pointer: water".into();
    }
    let all = w.survey_at(p);
    let value = |x: LandLayer| all.iter().find(|(y, _)| *y == x).map_or(0.0, |v| v.1);
    match l {
        LandLayer::BestIndustry => {
            let (k, v) = IndustryKind::ALL.iter().map(|k| (*k, value(LandLayer::Industry(*k)))).fold((IndustryKind::Manufacturing, -1.0), |a, b| if b.1 > a.1 { b } else { a });
            format!("under the pointer: {} ({:.0}% chance)", k.label(), v * 100.0)
        }
        LandLayer::Industry(_) => format!("under the pointer: {:.0}% chance", value(l) * 100.0),
        _ => format!("under the pointer: {:.0}%", value(l) * 100.0),
    }
}
