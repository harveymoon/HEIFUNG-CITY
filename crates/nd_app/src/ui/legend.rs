//! The map legend: a small panel at the bottom right of the map with the
//! colour key of the map layer that is on (`crate::map_layers`), drawn from the same ramps the
//! renderer uses (`nd_procgen::overlay`), so the two cannot drift apart.

use bevy_egui::egui::{self, Color32};
use nd_procgen::overlay::{industry_hue, land_color, land_hue};
use nd_sim::survey::LandLayer;
use nd_sim::IndustryKind;

use super::{hint, section};
use crate::assets::GameAssets;
use crate::map_layers::{self, ramp, LayerId, LegendCtx};
use crate::render::Overlays;

fn linear(c: [f32; 3]) -> Color32 {
    Color32::from(egui::Rgba::from_rgb(c[0].min(1.0), c[1].min(1.0), c[2].min(1.0)))
}

/// The key of the layer showing, if any, at the bottom right of the map:
/// its name, colour key and (for land layers) the reading under the pointer.
pub fn show(ctx: &egui::Context, o: &Overlays, assets: &GameAssets, world: &nd_sim::World, area: egui::Rect, here: Option<&str>) {
    let Some(layer) = map_layers::active(o) else { return };
    egui::Area::new(egui::Id::new("map_legend"))
        .order(egui::Order::Middle)
        .interactable(false)
        .fixed_pos(egui::pos2(area.right() - 246.0, area.bottom() - 12.0))
        .pivot(egui::Align2::LEFT_BOTTOM)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(Color32::from_rgba_unmultiplied(8, 10, 20, 225)).show(ui, |ui| {
                ui.set_width(216.0);
                if let LayerId::Flag(_) = layer {
                    section(ui, &layer.label().to_uppercase());
                }
                map_layers::legend(ui, layer, &LegendCtx { assets, world });
                if let (LayerId::Land(_), Some(t)) = (layer, here) {
                    ui.label(egui::RichText::new(t).small().color(Color32::from_rgb(230, 235, 255)));
                }
            });
        });
}

/// The key of the land survey layer: a ramp in its hue, or for "best
/// industry" a swatch per kind; then what the layer means and who wants it.
pub fn land_legend(ui: &mut egui::Ui, l: LandLayer) {
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
        LandLayer::Wind => {
            ramp(ui, |t| { let c = nd_procgen::overlay::wind_color(t); linear([c[0], c[1], c[2]]) }, "becalmed", "full output");
        }
        LandLayer::Solar => {
            let h = land_hue(l);
            ramp(ui, |t| { let c = land_color(h, t); linear([c[0], c[1], c[2]]) }, "weak sun", "desert sun");
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
    // [grid-weather] Wind and sun read in their own units.
    match l {
        LandLayer::Wind => {
            let s = w.site_resource_at(p);
            return format!("under the pointer: mean wind {:.1} m/s (site ×{:.2}); a wind farm makes ~{:.0}% on an ordinary day", nd_sim::weather::mean_wind_ms_at(&w.landscape, p), s.wind, nd_sim::weather::wind_yield_at(&w.landscape, p) * 100.0);
        }
        LandLayer::Solar => {
            return format!("under the pointer: sun {:.0}% of the desert's ({} climate)", nd_sim::weather::sun_at(&w.landscape, p) * 100.0, w.landscape.params.climate.label());
        }
        _ => {}
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
