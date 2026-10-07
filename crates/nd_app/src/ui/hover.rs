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
    let Some((_, lot)) = w.lots.iter().find(|(_, l)| point_in_polygon(ground, &l.polygon)) else {
        // [events] Off the lots, on a street: its condition.
        super::nights::road_card(ctx, w, ground, pointer);
        return;
    };
    let building = lot.building.and_then(|b| w.buildings.get(b));
    let value = w.land_value.at(lot.center);
    // [access] Walking time to parks and nightlife, and what it is worth to homes.
    let access = w.land_value.access_at(lot.center);
    // [free-placement] Under a city building the ground is out of the zoning.
    let zone = if w.is_city_ground(lot) {
        "city ground, out of the zoning".to_string()
    } else {
        w.zones.get(lot.zone).map_or_else(|| "no zone".to_string(), |z| format!("{} zone of {:.0} m²", z.land_use.label(), polygon_area(&z.polygon).abs()))
    };
    let small = |t: String| egui::RichText::new(t).small();

    // Keep the card on screen: flip to the other side of the pointer near the edges.
    let size = egui::vec2(300.0, if building.is_some() { 120.0 } else { 80.0 } + if access.is_some() { 18.0 } else { 0.0 });
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
                    ui.label(small(format!("{}  ·  {} tier  ·  {} floors{}", lot.building.map_or(b.land_use, |id| w.shown_use(id)).label(), b.tier.label(), b.floors, if b.is_derelict() { "  ·  derelict" } else { "" })));
                    ui.label(small(format!("homes {}/{}  ·  jobs {}/{}", b.households.len(), b.homes, b.workers.len(), b.jobs)));
                    if let Some(bz) = &b.business {
                        ui.label(small(format!("{}{}", bz.kind.label(), if bz.closed { "  (closed)" } else { "" })));
                    }
                }
                None => neon(ui, ground_label(lot)),
            }
            ui.label(small(format!("{}  ·  {zone}", lot.land_use.label())));
            ui.label(small(format!(
                "lot {:.0} m²  ·  land value {:.2} ({} tier land){}{}",
                lot.area(),
                value,
                Tier::for_land_value(value).label(),
                if lot.landlocked && !is_park(lot) { "  ·  landlocked" } else { "" },
                if lot.park.is_some() { "  ·  park" } else { "" }
            )));
            if let Some(a) = access {
                ui.label(small(format!("{}  ·  {:+.2} for homes", a.describe(), a.value)));
            }
        });
    });
}

/// A lot zoned for a park is a park, laid out or not, even where no street
/// reaches it: parks are walked into, so "unusable" only describes landlocked
/// ground nobody has given a use.
fn is_park(lot: &nd_sim::Lot) -> bool {
    lot.park.is_some() || lot.land_use == nd_sim::LandUse::Park
}

/// The card's title for a lot with no building on it.
pub fn ground_label(lot: &nd_sim::Lot) -> &'static str {
    if is_park(lot) {
        "Park"
    } else if lot.landlocked {
        "Unusable ground"
    } else {
        "Vacant lot"
    }
}
