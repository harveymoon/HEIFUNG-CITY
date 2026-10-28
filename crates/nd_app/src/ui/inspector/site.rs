//! [p2-look] The inspector on a strip mall, mall or mega-mall (its shops,
//! who has them and which are to let, its anchors and food court, its
//! owner, car park and how far it draws shoppers from) and on a site grown
//! over the plots next door (a strip mine, oil field, farm, car park or
//! works: the plots joined, the area, the owner).

use super::*;
use nd_sim::buildings::{Occupant, UnitRole};
use nd_sim::estate::Form;

/// A tenant's (or the building's own business's) name and trade, or "to
/// let" with the days it has stood empty.
fn occupant_text(w: &nd_sim::World, b: &nd_sim::Building, u: &nd_sim::buildings::Unit) -> (String, Option<nd_sim::BuildingId>) {
    match u.occupant {
        Occupant::Vacant => {
            let days = w.clock.tick.saturating_sub(u.vacant_since) / 1440;
            (format!("to let, {days} day{}", fmt::s(days)), None)
        }
        Occupant::Main => (format!("{} (its own {})", b.name, b.business.as_ref().map_or("store", |z| z.kind.label())), None),
        Occupant::Tenant(t) => match w.buildings.get(t) {
            Some(tb) => (format!("{} ({}){}", tb.name, tb.business.as_ref().map_or("", |z| z.kind.label()), if tb.business.as_ref().is_some_and(|z| z.closed) { ", shut" } else { "" }), Some(t)),
            None => ("?".to_string(), None),
        },
    }
}

/// The MALL section of a strip mall, mall or mega-mall. Returns a building
/// to select (a tenant clicked).
pub(super) fn mall_section(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId) -> Option<nd_sim::BuildingId> {
    let b = w.buildings.get(id)?;
    let form = b.plot.form.filter(|f| f.is_mall())?;
    if b.is_tenant() {
        return None;
    }
    let shops: Vec<&nd_sim::buildings::Unit> = b.units.iter().filter(|u| u.shopfront).collect();
    let let_n = shops.iter().filter(|u| !u.vacant()).count();
    section(ui, &format!("{}  ·  {let_n} of {} shops let", form.label().to_uppercase(), shops.len()));
    let anchors = shops.iter().filter(|u| u.role == UnitRole::Anchor).count();
    let food = shops.iter().filter(|u| u.role == UnitRole::FoodCourt).count();
    let tenants = w.tenants_of(id).len();
    ui.horizontal_wrapped(|ui| {
        figure(ui, "shops", format!("{}", shops.len() - anchors - food), IVORY);
        if anchors > 0 {
            figure(ui, "anchors", format!("{anchors}"), IVORY);
        }
        if food > 0 {
            figure(ui, "food court", format!("{food}"), IVORY);
        }
        figure(ui, "let", format!("{let_n}"), GOOD);
        figure(ui, "to let", format!("{}", shops.len() - let_n), if shops.len() > let_n { WARN } else { IVORY });
        figure(ui, "tenants", format!("{tenants}"), IVORY);
    });
    ui.label(egui::RichText::new(format!("owned by {}", crate::ui::estate::owner_text(w, id))).color(IVORY));
    // The car park, and how far it draws shoppers from.
    let lot = match form {
        Form::StripMall => "car park out front",
        Form::Mall => "car park round it",
        _ => "parking structure (3 decks) and a surface car park",
    };
    let reach = w.shopper_reach_m(id);
    hint(ui, &format!("{}: {} spaces, {} taken  ·  draws shoppers from about {}{}", capitalise(lot), b.parking_capacity, b.parking_used, fmt_km(reach), if w.shop_reach(id) > 1.0 { format!(" ({:.1}× a shop on the street)", w.shop_reach(id)) } else { String::new() }));
    if b.plot.merged() {
        hint(ui, &format!("on {} plots, {:.0} m²", 1 + b.plot.extra_lots.len(), w.footprint(id).map_or(0.0, |l| l.area())));
    }
    // Who has what: anchors first, then the food court, then the shops.
    let mut pick = None;
    for (role, title) in [(UnitRole::Anchor, "anchor stores"), (UnitRole::FoodCourt, "food court"), (UnitRole::Shop, "shops")] {
        let mine: Vec<&&nd_sim::buildings::Unit> = shops.iter().filter(|u| u.role == role).collect();
        if mine.is_empty() {
            continue;
        }
        ui.label(egui::RichText::new(title).small().color(GOLD));
        egui::Grid::new(("inspector_mall", id, title)).num_columns(2).spacing([8.0, 1.0]).show(ui, |ui| {
            for u in mine {
                ui.label(egui::RichText::new(u.place()).small().weak());
                let (text, link) = occupant_text(w, b, u);
                match link {
                    Some(t) => {
                        if ui.link(egui::RichText::new(text).small()).clicked() {
                            pick = Some(t);
                        }
                    }
                    None => {
                        ui.label(egui::RichText::new(text).small().color(if u.vacant() { WARN } else { GOLD }));
                    }
                }
                ui.end_row();
            }
        });
    }
    let offices = b.units.iter().filter(|u| !u.shopfront).count();
    if offices > 0 {
        let let_o = b.units.iter().filter(|u| !u.shopfront && !u.vacant()).count();
        hint(ui, &format!("upstairs: {let_o} of {offices} office floor{} let", fmt::s(offices)));
    }
    pick
}

/// "850 m" or "1.4 km".
fn fmt_km(m: f32) -> String {
    if m >= 1000.0 {
        format!("{:.1} km", m / 1000.0)
    } else {
        format!("{:.0} m", m)
    }
}

/// The SITE section of a business grown over the plots next door (a pit,
/// a field, a well, a car park, works): the plots joined (the backland
/// among them), the area, the owner, how far it may still grow.
pub(super) fn joined_site_section(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId) {
    let Some(b) = w.buildings.get(id) else { return };
    if b.plot.extra_lots.is_empty() || b.plot.form.is_some_and(|f| f != Form::Merged) || b.is_tenant() {
        return;
    }
    let Some(kind) = b.business.as_ref().map(|z| z.kind) else { return };
    let plots = 1 + b.plot.extra_lots.len();
    let back = b.plot.extra_lots.iter().filter(|l| w.lots.get(**l).is_some_and(|x| x.landlocked)).count();
    let area = w.footprint(id).map_or(0.0, |l| l.area());
    section(ui, &format!("SITE  ·  {plots} plots joined"));
    ui.horizontal_wrapped(|ui| {
        figure(ui, "plots", format!("{plots}"), IVORY);
        if back > 0 {
            figure(ui, "backland", format!("{back}"), IVORY);
        }
        figure(ui, "area", format!("{:.0} m²", area), IVORY);
        if b.is_parking() {
            figure(ui, "bays", format!("{}", b.parking_capacity), IVORY);
        }
    });
    ui.label(egui::RichText::new(format!("owned by {}", crate::ui::estate::owner_text(w, id))).color(IVORY));
    let cap = nd_sim::estate::site_cap(kind);
    let what = if kind == nd_sim::BusinessKind::StripMine {
        // The pit as drawn: across its long and short ways.
        let pit = w.footprint(id).and_then(|l| nd_procgen::strip_mine_hole(&l));
        pit.map_or(String::new(), |p| {
            let (lo, hi) = p.iter().fold((nd_sim::Vec2::splat(f32::MAX), nd_sim::Vec2::splat(f32::MIN)), |(lo, hi), q| (lo.min(*q), hi.max(*q)));
            let d = hi - lo;
            format!("the pit about {:.0} × {:.0} m  ·  ", d.x.max(d.y), d.x.min(d.y))
        })
    } else {
        String::new()
    };
    let pending = b.plot.pending_lots.len();
    hint(ui, &format!("{what}a {} spread over the plots behind and beside its road plot{}  ·  may grow to {cap} plots{}", kind.label(), if back > 0 { ", the backland among them" } else { "" }, if pending > 0 { format!("  ·  {pending} bought plot{} still clearing", fmt::s(pending)) } else { String::new() }));
}
