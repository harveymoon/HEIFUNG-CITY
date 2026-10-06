//! [places] Read-outs for taste and memory of places: a citizen's favourite
//! spots and the places they swore off, and a venue's character and name
//! about town.

use bevy_egui::egui;
use nd_sim::{BuildingId, CitizenId, World};

use super::hint;

/// Favourite places (regular spots first), places sworn off, and people
/// known from the regular haunts.
pub fn citizen_places(ui: &mut egui::Ui, w: &World, id: CitizenId) {
    let Some(c) = w.citizens.get(id) else { return };
    if c.places.places.is_empty() {
        return;
    }
    let name = |b: BuildingId| w.buildings.get(b).map_or("(gone)", |b| b.name.as_str());
    hint(ui, &format!("places ({} known)", c.places.places.len()));
    for (b, m) in w.favourite_places(id).iter().take(5) {
        let tag = if m.regular { "regular" } else if m.score >= 0.4 { "liked" } else if m.score < 0.0 { "meh" } else { "been" };
        ui.label(egui::RichText::new(format!("{}  ·  {tag}  ·  {} visit{}  ·  {:+.2}", name(*b), m.visits, if m.visits == 1 { "" } else { "s" }, m.score)).small());
    }
    let never: Vec<&str> = c.places.places.iter().filter(|p| p.never).map(|p| name(p.place)).collect();
    if !never.is_empty() {
        ui.label(egui::RichText::new(format!("never again: {}", never.join(", "))).small().color(super::WARN));
    }
    let known: Vec<&str> = c.places.acquaintances.iter().filter_map(|a| w.citizens.get(a.who)).map(|x| x.name.as_str()).take(4).collect();
    if !known.is_empty() {
        ui.label(egui::RichText::new(format!("knows {} from the regular haunts", known.join(", "))).small());
    }
}

/// What the place is like and what the town thinks of it.
pub fn building_place(ui: &mut egui::Ui, w: &World, id: BuildingId) {
    let Some(ch) = w.place_character(id) else { return };
    hint(ui, &format!("character: {}", ch.summary()));
    ui.label(
        egui::RichText::new(format!(
            "polish {:.0}%  ·  price {:.0}%  ·  noise {:.0}%  ·  crowd {:.0}%  ·  safety {:.0}%  ·  clean {:.0}%",
            ch.polish * 100.0,
            ch.price * 100.0,
            ch.noise * 100.0,
            ch.crowd * 100.0,
            ch.safety * 100.0,
            ch.clean * 100.0
        ))
        .small(),
    );
    match w.reputation(id) {
        Some(r) if r.rated > 0 => {
            ui.label(format!("rating {:.1}★ from {} visits  ·  {} regulars  ·  {} never again", r.stars(), r.rated, r.regulars, r.never_again));
        }
        _ => {
            ui.label(egui::RichText::new("no word on it yet").small().weak());
        }
    }
}
