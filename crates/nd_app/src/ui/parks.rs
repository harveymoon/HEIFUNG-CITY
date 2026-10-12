//! [parks] (D5) City Data, City, Parks: every park with how well kept it
//! is and what wears it down, a paid clean-up and the nature-preserve
//! switch; and the same in the inspector of a park's building.

use bevy::math::Vec2;
use bevy_egui::egui;
use nd_sim::parks::ParkState;
use nd_sim::BuildingId;

use super::pane::{cards, heading, kpi};
use super::theme::GOOD;
use super::{hint, Selection, UiCtx, GOLD, IVORY, WARN};

/// A park's state as a coloured word.
fn kept_word(s: &ParkState) -> egui::RichText {
    let pct = (s.kept * 100.0).round();
    if s.preserve {
        egui::RichText::new(format!("wild, {pct:.0}%")).color(GOOD)
    } else if s.run_down() {
        egui::RichText::new(format!("run down, {pct:.0}%")).color(WARN)
    } else if s.kept < 0.75 {
        egui::RichText::new(format!("worn, {pct:.0}%")).color(GOLD)
    } else {
        egui::RichText::new(format!("kept, {pct:.0}%")).color(GOOD)
    }
}

/// The buttons for one park: clean it up, leave it wild (or not). Returns
/// a message to show, if one was pressed.
fn actions(ui: &mut egui::Ui, w: &mut nd_sim::World, id: BuildingId, s: &ParkState) -> Option<String> {
    let mut msg = None;
    let cost = s.cleanup_cost();
    let dirty = s.trash >= 0.1 || s.dealers > 0 || s.campers > 0;
    if ui.add_enabled(dirty && !s.preserve, egui::Button::new(format!("Clean up ({cost:.0})"))).on_hover_text("A crew clears the litter, moves the dealers on (they keep away for ten days) and finds the people camping by it somewhere else to go").clicked() {
        msg = Some(match w.clean_up_park(id) {
            Ok((_, m)) => m,
            Err(e) => format!("Cannot: {e}."),
        });
    }
    let label = if s.preserve { "Make it a kept park" } else { "Leave it wild" };
    if ui.button(label).on_hover_text("A nature preserve: meadow and woodland with a trail, no lawns, rides or courts; little litter and no dealers").clicked() {
        msg = Some(match w.set_park_preserve(id, !s.preserve) {
            Ok(m) => m,
            Err(e) => format!("Cannot: {e}."),
        });
    }
    msg
}

/// The inspector's PARK section for a park building (any lot of a park).
pub fn inspector(ui: &mut egui::Ui, sim: &mut crate::sim::Sim, id: BuildingId) -> Option<String> {
    let w = &sim.world;
    let a = w.parks.anchor(id)?;
    let s = w.parks.parks.get(&a)?.clone();
    super::pane::heading(ui, "PARK");
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&s.name).color(IVORY).strong());
        ui.label(kept_word(&s));
    });
    hint(ui, &format!("{} lot{}, {:.0} m² · {} visitor-hours yesterday", s.lots.len().max(1), if s.lots.len() > 1 { "s" } else { "" }, s.area, s.visits_yesterday));
    let t = s.troubles();
    if !t.is_empty() {
        ui.label(egui::RichText::new(t.join(" · ")).color(if s.run_down() { WARN } else { GOLD }).small());
    }
    if s.run_down() {
        hint(ui, "Run down: it sours the land round it, and people would rather sit elsewhere.");
    }
    if s.cleanups > 0 {
        hint(ui, &format!("cleaned up {} time{}", s.cleanups, if s.cleanups == 1 { "" } else { "s" }));
    }
    let mut msg = None;
    ui.horizontal(|ui| msg = actions(ui, &mut sim.world, a, &s));
    msg
}

/// City Data, City, Parks.
pub fn page(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let parks: Vec<(BuildingId, ParkState)> = w.parks.parks.iter().map(|(a, s)| (*a, s.clone())).collect();
    let run_down = parks.iter().filter(|(_, s)| s.run_down()).count();
    let area: f32 = parks.iter().map(|(_, s)| s.area).sum();
    cards(ui, |ui| {
        kpi(ui, "parks", egui::RichText::new(format!("{}", parks.len())));
        kpi(ui, "ground", egui::RichText::new(format!("{:.1} ha", area / 10_000.0)));
        kpi(ui, "run down", super::tools::warn_if(run_down > 0, format!("{run_down}")));
        kpi(ui, "nature preserves", egui::RichText::new(format!("{}", parks.iter().filter(|(_, s)| s.preserve).count())));
        kpi(ui, "dealers", super::tools::warn_if(parks.iter().any(|(_, s)| s.dealers > 0), format!("{}", parks.iter().map(|(_, s)| s.dealers as u32).sum::<u32>())));
        kpi(ui, "visitor-hours yesterday", egui::RichText::new(format!("{}", parks.iter().map(|(_, s)| s.visits_yesterday).sum::<u32>())));
    });
    hint(ui, "Litter builds up where people go and no trash depot reaches; parks on cheap land the police seldom reach draw dealers, worse with litter and people sleeping rough. A run-down park sours the land round it. A clean-up clears it for a while; a nature preserve stays wild and draws no dealers.");
    if parks.is_empty() {
        hint(ui, "No parks yet: zone Park land along a street.");
        return;
    }
    heading(ui, "Every park, the worst kept first");
    let mut order = parks;
    order.sort_by(|a, b| a.1.kept.total_cmp(&b.1.kept).then(a.0.cmp(&b.0)));
    let mut msg = None;
    egui::Grid::new("parks_grid").striped(true).num_columns(6).show(ui, |ui| {
        for h in ["park", "size", "state", "what wears it", "visits", ""] {
            ui.label(egui::RichText::new(h).small().strong());
        }
        ui.end_row();
        for (a, s) in &order {
            if ui.link(&s.name).on_hover_text("Inspect it and fly there").clicked() {
                *p.selection = Selection::Building(*a);
                if let Some(c) = p.sim.world.buildings.get(*a).and_then(|b| p.sim.world.lots.get(b.lot)).map(|l| l.center) {
                    p.fly_to = Some(Vec2::new(c.x, c.y));
                }
            }
            ui.label(egui::RichText::new(format!("{:.0} m²", s.area)).small());
            ui.label(kept_word(s).small());
            let t = s.troubles();
            ui.label(egui::RichText::new(if t.is_empty() { "-".to_string() } else { t.join(", ") }).small());
            ui.label(egui::RichText::new(format!("{}", s.visits_yesterday)).small());
            ui.horizontal(|ui| {
                if let Some(m) = actions(ui, &mut p.sim.world, *a, s) {
                    msg = Some(m);
                }
            });
            ui.end_row();
        }
    });
    if let Some(m) = msg {
        p.flags.message = m;
    }
}
