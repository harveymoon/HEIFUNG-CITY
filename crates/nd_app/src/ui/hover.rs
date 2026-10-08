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

/// [plant-groups] The Buildings tool's card beside the pointer: what the
/// picked building would be on the lot under it before the click. Plants
/// show their MW, cost and upkeep for this lot (solar and wind are sized
/// to it) and the Sunfield or Wind Park they would join; anything else its
/// cost; and why it cannot go, if it cannot.
pub fn build_card(ctx: &egui::Context, sim: &Sim, cursor: &Cursor, tools: &crate::tools::ToolState) {
    let what = tools.build_item();
    use nd_sim::Placeable;
    if matches!(what, Placeable::Toll(_)) {
        return;
    }
    let (Some(ground), Some(pointer)) = (cursor.ground, ctx.pointer_latest_pos()) else { return };
    let w = &sim.world;
    // [dock-access] Docks and rail pieces: no lot and no road needed; the
    // verdict says what it would lay to be joined up later.
    if let Some(v) = w.piece_verdict(what, ground) {
        let small = |t: String| egui::RichText::new(t).small();
        let mut lines = vec![small(format!("costs {:.0}  ·  {:.0}/day", what.cost(), what.upkeep()))];
        match v {
            Ok(note) => {
                lines.push(small(note).color(egui::Color32::from_rgb(120, 230, 150)));
                lines.push(small("click to build".to_string()).color(egui::Color32::from_rgb(120, 230, 150)));
            }
            Err(e) => lines.push(small(format!("cannot build: {e}")).color(egui::Color32::from_rgb(255, 120, 110))),
        }
        show_card(ctx, pointer, &format!("{} here", capitalise(&what.label())), lines);
        return;
    }
    // [plant-scale] Any lot, or the site on unzoned ground a click there
    // would make (`World::placement_site`), as the click sees it.
    // The site the tool worked out this frame while the pointer stays on
    // its parcel (joining unzoned parcels is the dear part).
    let key = w.parcels.at(ground).map(|i| (what, i, w.parcels.version, w.lots.len()));
    let site = match (&tools.build_site, key) {
        (Some((a, b, c, d, s)), Some(k)) if (*a, *b, *c, *d) == k => s.clone(),
        _ => w.placement_site(what, ground),
    };
    let Ok(site) = site else { return };
    let area = polygon_area(&site.outline).abs();
    let small = |t: String| egui::RichText::new(t).small();
    let good = egui::Color32::from_rgb(120, 230, 150);
    let bad = egui::Color32::from_rgb(255, 120, 110);
    let mut lines: Vec<egui::RichText> = Vec::new();
    let title;
    let problem;
    if let Placeable::Plant(k) = what {
        let Some(p) = w.plant_preview_site(k, &site) else { return };
        title = format!("{} on {:.0} m²{}", capitalise(k.label()), p.area, if site.lot.is_none() { " (unzoned)" } else { "" });
        let site_f = if p.site > 0.0 && (p.site - 1.0).abs() > 0.01 { format!("  ·  site {:.0}%", p.site * 100.0) } else { String::new() };
        lines.push(small(format!("{:.1} MW nameplate{site_f}  ·  costs {:.0}  ·  {:.0}/day", p.capacity_mw, p.cost, p.upkeep_per_day)));
        lines.push(small(format!("about {:.1} MW right now (weather, hour and site)", p.now_mw)).weak());
        if let Some(a) = k.reference_area() {
            let (_, top) = k.size_range();
            let capped = if p.scale >= top - 1e-3 && k.sized_when_built() { "  ·  its largest: the rest is yard" } else { "" };
            lines.push(small(format!("{:.2}× a standard {:.0} m² {}{capped}", p.scale, a, k.label())).weak());
        }
        if let Some(j) = &p.join {
            let name = j.joins.clone().unwrap_or_default();
            lines.push(small(format!("joins {name}: {} lots, {:.0} m², {:.0} MW together (+{:.0} MW)", j.members.len() + 1, j.area, j.capacity_mw, j.capacity_mw - j.before_mw)).color(good));
            if p.small_lot_joins {
                lines.push(small(format!("too small alone ({:.0} m² needed): allowed as part of {name}", k.min_area())).color(good));
            }
        }
        problem = p.problem;
    } else {
        title = format!("{} here", capitalise(&what.label()));
        lines.push(small(format!("costs {:.0}  ·  {:.0}/day  ·  lot {area:.0} m²", what.cost(), what.upkeep())));
        problem = w.site_problem(what, &site);
    }
    match &problem {
        Some(e) => lines.push(small(format!("cannot build: {e}")).color(bad)),
        None => lines.push(small("click to build".to_string()).color(good)),
    }
    show_card(ctx, pointer, &title, lines);
}

/// The card beside the pointer: a title and its lines.
fn show_card(ctx: &egui::Context, pointer: egui::Pos2, title: &str, lines: Vec<egui::RichText>) {
    let size = egui::vec2(300.0, 20.0 + 16.0 * lines.len() as f32);
    let view = ctx.viewport_rect();
    let mut at = pointer + egui::vec2(18.0, 18.0);
    if at.x + size.x > view.right() {
        at.x = pointer.x - 18.0 - size.x;
    }
    if at.y + size.y > view.bottom() {
        at.y = pointer.y - 18.0 - size.y;
    }
    egui::Area::new(egui::Id::new("build_hover_card")).order(egui::Order::Tooltip).interactable(false).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_max_width(size.x);
            neon(ui, title);
            for l in lines {
                ui.label(l);
            }
        });
    });
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}
