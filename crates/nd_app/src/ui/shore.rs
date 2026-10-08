//! [waterfront] The inspector's shore lines: a building's edge on the
//! water, a public beach's state (access, the crowd, the weather, a closed
//! sign) and a business's private jetty.

use bevy_egui::egui;
use nd_sim::{BuildingId, World};

use super::{hint, section, WARN};

const SAND: egui::Color32 = egui::Color32::from_rgb(240, 210, 140);
const SEA: egui::Color32 = egui::Color32::from_rgb(110, 200, 240);

/// The WATERFRONT / PUBLIC BEACH section of a building's inspector.
pub fn lines(ui: &mut egui::Ui, w: &World, id: BuildingId) {
    let Some(b) = w.buildings.get(id) else { return };
    let Some(lot) = w.lots.get(b.lot) else { return };
    if let Some(s) = w.shore.beach(id) {
        section(ui, "PUBLIC BEACH");
        if !s.access {
            ui.label(egui::RichText::new("NO ACCESS").color(WARN).strong());
            hint(ui, "Nobody can reach the sand: connect a path or a road to it, or a park next to it.");
        } else if !s.open {
            ui.label(egui::RichText::new("CLOSED: THE WATER IS FOULED").color(WARN).strong());
            hint(ui, &format!("Contamination {:.0}% over the sand and the water off it (closes at {:.0}%, opens again under {:.0}%). Clean up the dirty industry upstream.", s.contamination * 100.0, nd_sim::shore::BEACH_CLOSE_AT * 100.0, nd_sim::shore::BEACH_REOPEN_AT * 100.0));
        } else {
            let crowd = if s.crowded() { "  ·  crowded" } else { "" };
            ui.label(egui::RichText::new(format!("{} on the sand, {} swimming{crowd}", s.visitors, s.swimmers)).color(SAND));
            hint(ui, &format!("beach weather now {:.0}% · comfortable for {} · {} residents within reach · {} of the city's own people there", s.appeal * 100.0, s.capacity, s.catchment, s.people_here));
            if let Some(n) = &s.note {
                hint(ui, n);
            }
        }
        hint(ui, &format!("today: {} bather-hours, {} at most at once · yesterday: {}", s.visits_today, s.peak_today, s.visits_yesterday));
        return;
    }
    if lot.shore.is_waterfront() {
        section(ui, "WATERFRONT");
        ui.label(egui::RichText::new(format!("{:.0} m on the water", lot.shore.frontage)).color(SEA));
        if let Some(j) = w.shore.jetties.get(&id) {
            let loads = if j.kind == nd_sim::shore::JettyKind::Cargo { format!(" · {} loads landed by its own coasters, {} on the way", j.loads, w.shore.inbound.iter().filter(|(x, _, _)| *x == id).count()) } else { String::new() };
            hint(ui, &format!("its own {} on the {}{loads}", j.kind.label(), j.water.label()));
        } else if let Some(k) = b.business.as_ref().and_then(|z| World::jetty_kind_for(z.kind)) {
            hint(ui, &format!("no {} yet: it builds one when it has the money and boats can work the water off it", k.label()));
        }
    }
}
