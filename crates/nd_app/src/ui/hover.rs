//! The parcel read-out beside the pointer while the Inspect tool is active.

use bevy_egui::egui;
use nd_sim::zoning::{point_in_polygon, polygon_area};
use nd_sim::Tier;

use super::neon;
use crate::camera::Cursor;
use crate::sim::Sim;

/// A read-out of the parcel under the cursor, beside the pointer. The area
/// is not interactable, so egui never counts the pointer as over it and the
/// click still reaches the Inspect tool.
pub fn card(ctx: &egui::Context, sim: &Sim, cursor: &Cursor) {
    let (Some(ground), Some(pointer)) = (cursor.ground, ctx.pointer_latest_pos()) else { return };
    let w = &sim.world;
    let Some((_, lot)) = w.lots.iter().find(|(_, l)| point_in_polygon(ground, &l.polygon)) else { return };
    let building = lot.building.and_then(|b| w.buildings.get(b));
    let value = w.land_value.at(lot.center);
    let zone = w.zones.get(lot.zone).map_or_else(|| "no zone".to_string(), |z| format!("{} zone of {:.0} m²", z.land_use.label(), polygon_area(&z.polygon).abs()));
    let small = |t: String| egui::RichText::new(t).small();

    // Keep the card on screen: flip to the other side of the pointer near the edges.
    let size = egui::vec2(300.0, if building.is_some() { 120.0 } else { 80.0 });
    let view = ctx.viewport_rect();
    let mut at = pointer + egui::vec2(18.0, 18.0);
    if at.x + size.x > view.right() {
        at.x = pointer.x - 18.0 - size.x;
    }
    if at.y + size.y > view.bottom() {
        at.y = pointer.y - 18.0 - size.y;
    }
    egui::Area::new(egui::Id::new("lot_hover_card")).order(egui::Order::Tooltip).interactable(false).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_max_width(size.x);
            match building {
                Some(b) => {
                    neon(ui, &b.name);
                    ui.label(small(format!("{}  ·  {} tier  ·  {} floors{}", b.land_use.label(), b.tier.label(), b.floors, if b.is_derelict() { "  ·  derelict" } else { "" })));
                    ui.label(small(format!("homes {}/{}  ·  jobs {}/{}", b.households.len(), b.homes, b.workers.len(), b.jobs)));
                    if let Some(bz) = &b.business {
                        ui.label(small(format!("{}{}", bz.kind.label(), if bz.closed { "  (closed)" } else { "" })));
                    }
                }
                None => neon(ui, if lot.landlocked { "Unusable ground" } else if lot.park.is_some() { "Park" } else { "Vacant lot" }),
            }
            ui.label(small(format!("{}  ·  {zone}", lot.land_use.label())));
            ui.label(small(format!(
                "lot {:.0} m²  ·  land value {:.2} ({} tier land){}{}",
                lot.area(),
                value,
                Tier::for_land_value(value).label(),
                if lot.landlocked { "  ·  landlocked" } else { "" },
                if lot.park.is_some() { "  ·  park" } else { "" }
            )));
        });
    });
}
