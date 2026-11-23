//! [r14-builders] The building trade in the inspector: a site's builder
//! ("built by Okafor Construction"), a contractor's jobs, crews and yard
//! stock, a builders' merchant's trade counter.

use super::*;

/// Under a site's progress bar: who is building it and for how much.
/// Returns the contractor's yard when its name was clicked.
pub(super) fn site_builder(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId) -> Option<nd_sim::BuildingId> {
    let c = w.buildings.get(id)?.construction.as_ref()?;
    let Some(k) = &c.contract else {
        hint(ui, "not yet let to a builder: it goes out to tender when the work starts");
        return None;
    };
    let mut pick = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new("Built by").small());
        match &k.builder {
            nd_sim::SiteBuilder::Local(yard) => {
                let name = w.buildings.get(*yard).map_or("a builder since gone", |b| b.name.as_str());
                if ui.link(egui::RichText::new(name).small().color(GOLD)).clicked() {
                    pick = Some(*yard);
                }
                ui.label(egui::RichText::new("(a contractor in town: its own crew)").small().weak());
            }
            nd_sim::SiteBuilder::OutOfTown { name, .. } => {
                ui.label(egui::RichText::new(name).small().color(GOLD));
                ui.label(egui::RichText::new("(from out of town: crews drive in, at a markup)").small().weak());
            }
        }
    });
    hint(ui, &format!("contract {:.0}, {:.0} paid so far", k.price, k.paid));
    pick
}

/// A contractor's section: crews, its sites now (links), its lorries and
/// what stands in its yard. Returns a site when one was clicked.
pub(super) fn contractor(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId) -> Option<nd_sim::BuildingId> {
    let b = w.buildings.get(id)?;
    let z = b.business.as_ref()?;
    let crews = w.contractor_crews(id);
    let sites = w.contractor_sites(id);
    section(ui, "BUILDER");
    ui.horizontal_wrapped(|ui| {
        figure(ui, "crews", format!("{crews}"), IVORY);
        figure(ui, "jobs on", format!("{}", sites.len()), if sites.len() as u16 >= crews && crews > 0 { WARN } else { IVORY });
        figure(ui, "own lorries", format!("{}", nd_sim::builders::lorries(crews)), IVORY);
    });
    hint(ui, &format!("crews of {} of its staff; takes a site when one is free, within {:.1} km", nd_sim::builders::CREW_SIZE, nd_sim::builders::LOCAL_REACH / 1000.0));
    let mut pick = None;
    if sites.is_empty() {
        hint(ui, "no jobs on: its crews wait at the yard (and it pays them all the same)");
    }
    for s in sites {
        let Some(sb) = w.buildings.get(*s) else { continue };
        let pct = sb.construction.as_ref().map_or(100.0, |c| c.progress * 100.0);
        ui.horizontal(|ui| {
            if ui.link(egui::RichText::new(&sb.name).small()).clicked() {
                pick = Some(*s);
            }
            ui.label(egui::RichText::new(format!("{pct:.0}% built, {} floor{}", sb.floors, fmt::s(sb.floors))).small().weak());
        });
    }
    let stock: Vec<String> = nd_sim::catalogue::site_materials().into_iter().map(|r| format!("{} {:.0}", r.label(), z.stock_of(r))).collect();
    hint(ui, &format!("yard stock: {}", stock.join(", ")));
    pick
}

/// A merchant's trade counter: each material's stock and trade price.
pub(super) fn merchant(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId) {
    let Some(b) = w.buildings.get(id) else { return };
    let Some(z) = &b.business else { return };
    section(ui, "TRADE COUNTER");
    for r in nd_sim::catalogue::site_materials() {
        let inbound = z.inbound_of(r);
        let on_way = if inbound > 0.5 { format!(", {inbound:.0} on the way") } else { String::new() };
        ui.label(egui::RichText::new(format!("{}: {:.0} in stock at {:.1}{on_way}", r.label(), z.stock_of(r), z.sell_price(r, b.jobs))).small());
    }
    hint(ui, "sells to the builders, the sites and any trade restocking; DIY and garden goods to residents");
}
