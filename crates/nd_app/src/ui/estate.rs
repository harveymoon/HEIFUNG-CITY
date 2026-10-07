//! [land] The inspector's OWNERSHIP section: who owns a building, what it
//! would sell for, the plots it spans and the form it was built in, the
//! owner's other properties (each a click away; the map highlights them
//! all while it is selected), and what the pressure on its lot allows.

use bevy_egui::egui;
use super::{hint, section, GOLD, IVORY};
use crate::tools::Selection;
use nd_sim::estate::OwnerKind;

fn money(v: f64) -> String {
    if v >= 1e9 {
        format!("${:.1}B", v / 1e9)
    } else if v >= 1e6 {
        format!("${:.1}M", v / 1e6)
    } else if v >= 1e3 {
        format!("${:.0}k", v / 1e3)
    } else {
        format!("${v:.0}")
    }
}

pub fn building_section(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId, selection: &mut Selection) {
    let Some(b) = w.buildings.get(id) else { return };
    if b.is_special() || b.land_use == nd_sim::LandUse::Park {
        return;
    }
    section(ui, "OWNERSHIP");
    let owner = w.owner_of(id);
    let who = match owner.map(|(_, o)| (o.name.clone(), &o.kind)) {
        Some((name, OwnerKind::Elite { .. })) => format!("{name} (billionaire)"),
        Some((name, OwnerKind::Developer)) => format!("{name} (developer)"),
        Some((name, OwnerKind::Household { .. })) => format!("{name} (owner-occupied)"),
        Some((name, OwnerKind::Person { .. })) => format!("{name}, who founded it"),
        Some((name, OwnerKind::Firm { kind })) => {
            let n = owner.map_or(0, |(_, o)| o.properties.len());
            if n > 1 { format!("{name} ({} chain of {n})", kind.label()) } else { format!("{name} (the {} firm)", kind.label()) }
        }
        None => if b.homes > 0 { "an independent landlord".to_string() } else { "independently owned".to_string() },
    };
    ui.label(egui::RichText::new(format!("owned by {who}  ·  for sale at {}", money(w.sale_price(id)))).color(IVORY));
    let mut bits: Vec<String> = Vec::new();
    if let Some(f) = b.plot.form {
        bits.push(f.label().to_string());
    }
    if b.plot.merged() {
        bits.push(format!("spans {} plots", 1 + b.plot.extra_lots.len()));
    }
    if !b.plot.pending_lots.is_empty() {
        bits.push(format!("{} bought plot{} still clearing", b.plot.pending_lots.len(), if b.plot.pending_lots.len() == 1 { "" } else { "s" }));
    }
    if let Some(lot) = w.lots.get(b.lot) {
        let p = w.estate.pressure.at(lot.center);
        bits.push(format!("pressure {p:.0} supports {} floors here (height cap {})", w.pressure_floors(b.lot), w.height_cap(b.lot)));
    }
    if w.wants_to_expand(id) {
        bits.push("doing well enough to expand".to_string());
    }
    hint(ui, &bits.join("  ·  "));
    let others: Vec<nd_sim::BuildingId> = w.owned_with(id).into_iter().filter(|p| *p != id).collect();
    if !others.is_empty() {
        ui.label(egui::RichText::new(format!("also owns {} other propert{} (highlighted on the map):", others.len(), if others.len() == 1 { "y" } else { "ies" })).small().color(GOLD));
        ui.horizontal_wrapped(|ui| {
            for p in others.iter().take(12) {
                let Some(o) = w.buildings.get(*p) else { continue };
                let kind = if o.homes > 0 && o.business.is_none() { "home" } else { o.business.as_ref().map_or("building", |z| z.kind.label()) };
                if ui.small_button(format!("{} ({kind})", o.name)).clicked() {
                    *selection = Selection::Building(*p);
                }
            }
        });
    }
}

/// The super-elite line in a citizen's panel: fortune, mansion, garage
/// and the convoy.
pub fn citizen_line(ui: &mut egui::Ui, w: &nd_sim::World, cid: nd_sim::CitizenId) {
    let Some(e) = w.estate.elite_of(cid) else { return };
    let mansion = e.mansion.and_then(|m| w.buildings.get(m)).map_or("no mansion yet".to_string(), |m| if m.construction.is_some() { format!("{} (being built)", m.name) } else { m.name.clone() });
    let props = w.estate.owners.get(e.owner).map_or(0, |o| o.properties.len());
    ui.label(egui::RichText::new(format!("SUPER-ELITE  ·  worth {}  ·  {mansion}  ·  {props} properties  ·  {} supercars  ·  {} escort cars{}", money(e.net_worth), e.cars.len(), e.escorts, if e.police { " and a police escort" } else { "" })).strong().color(GOLD));
}
