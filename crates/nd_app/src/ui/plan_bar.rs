//! [fix/roads-tools] The plan bar (flag G2): while the map has changes not
//! yet implemented, a bar at the top of the map says so, with Implement,
//! Discard (asked twice) and Show. A plan drawn and forgotten tinted the
//! map with zoning nobody had built, and the only way to tell was the
//! small Project block on the toolbar.

use bevy_egui::egui;
use nd_sim::{Gesture, GestureId, Project};
use std::collections::BTreeMap;

use super::{theme, UiCtx, GOLD, IVORY, WARN};

/// What a project changes, in a few words: "2 roads, 1 roundabout, 14
/// parcels zoned".
pub fn summary(p: &Project) -> String {
    // The last version of each gesture the project sets (None: removed).
    let mut last: BTreeMap<GestureId, Option<&Gesture>> = BTreeMap::new();
    for step in &p.steps {
        for (id, g) in step {
            last.insert(*id, g.as_ref());
        }
    }
    let (mut roads, mut rings, mut zones, mut removed) = (0usize, 0usize, 0usize, 0usize);
    let mut parcels = std::collections::BTreeSet::new();
    for g in last.values() {
        match g {
            Some(Gesture::Road { .. }) => roads += 1,
            Some(Gesture::Roundabout { .. }) => rings += 1,
            Some(Gesture::Zone { .. }) => zones += 1,
            Some(Gesture::ParcelZone { parcels: ps, .. }) => parcels.extend(ps.iter().copied()),
            None => removed += 1,
        }
    }
    let mut parts: Vec<String> = Vec::new();
    let n = |k: usize, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
    if roads > 0 {
        parts.push(n(roads, "road", "roads"));
    }
    if rings > 0 {
        parts.push(n(rings, "roundabout", "roundabouts"));
    }
    if zones > 0 {
        parts.push(n(zones, "zone", "zones"));
    }
    if !parcels.is_empty() {
        parts.push(format!("{} zoned", n(parcels.len(), "parcel", "parcels")));
    }
    if removed > 0 {
        parts.push(format!("{} taken out", n(removed, "piece", "pieces")));
    }
    if parts.is_empty() {
        "changes".into()
    } else {
        parts.join(", ")
    }
}

/// Where the project's changes are, for Show: the middle of every point
/// it sets.
pub fn centre(p: &Project, w: &nd_sim::World) -> Option<nd_sim::Vec2> {
    let mut sum = nd_sim::Vec2::ZERO;
    let mut n = 0.0;
    let mut add = |q: nd_sim::Vec2| {
        sum += q;
        n += 1.0;
    };
    let plan = w.effective_plan();
    for id in p.touched() {
        match plan.gestures.get(&id).or_else(|| w.master.gestures.get(&id)) {
            Some(Gesture::ParcelZone { parcels, .. }) => {
                for k in parcels {
                    if let Some(i) = w.parcels.find(k) {
                        add(w.parcels.parcels[i].center);
                    }
                }
            }
            Some(g) => g.points().iter().for_each(|&q| add(q)),
            None => {}
        }
    }
    (n > 0.0).then(|| sum / n)
}

/// The bar, centred under the top bar, while the active project has
/// changes (or another project is pending).
pub fn show(ctx: &egui::Context, p: &mut UiCtx, top: f32) {
    let mut confirm_now = p.flags.plan_confirm;
    let confirm = &mut confirm_now;
    show_inner(ctx, p, top, confirm);
    p.flags.plan_confirm = confirm_now;
}

fn show_inner(ctx: &egui::Context, p: &mut UiCtx, top: f32, confirm: &mut bool) {
    let w = &p.sim.world;
    let active = w.project.as_ref().filter(|pr| !pr.is_empty());
    let parked = w.parked_projects.iter().filter(|pr| !pr.is_empty()).count();
    let Some(pr) = active else {
        *confirm = false;
        if parked > 0 {
            egui::Area::new(egui::Id::new("plan_bar")).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, top + 6.0)).show(ctx, |ui| {
                frame().show(ui, |ui| {
                    ui.label(egui::RichText::new(format!("{parked} parked project{} pending: pick one in the Project list to implement it", if parked == 1 { "" } else { "s" })).color(IVORY));
                });
            });
        }
        return;
    };
    let (id, name, what) = (pr.id, pr.name.clone(), summary(pr));
    let at = centre(pr, w);
    let refusal = p.tools.plan_check.as_ref().and_then(|c| c.refusal.clone());
    let mut implement = false;
    let mut discard = false;
    egui::Area::new(egui::Id::new("plan_bar")).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, top + 6.0)).show(ctx, |ui| {
        frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("\u{25cf} NOT BUILT YET").strong().color(GOLD));
                ui.label(egui::RichText::new(format!("{name}: {what}")).color(IVORY));
                if parked > 0 {
                    ui.label(egui::RichText::new(format!("(+{parked} parked)")).small().weak());
                }
                ui.separator();
                let imp = egui::Button::new(egui::RichText::new("Implement").strong().font(theme::button()).color(egui::Color32::WHITE)).fill(egui::Color32::from_rgb(40, 120, 70));
                if ui.add(imp).on_hover_text("Build it now (Ctrl+Enter)").clicked() {
                    implement = true;
                }
                if *confirm {
                    ui.label(egui::RichText::new("Throw it away?").color(WARN));
                    if ui.button(egui::RichText::new("Discard").color(WARN)).clicked() {
                        discard = true;
                    }
                    if ui.button("Keep").clicked() {
                        *confirm = false;
                    }
                } else if ui.button("Discard").on_hover_text("Throw the whole project away (asks again)").clicked() {
                    *confirm = true;
                }
                if let Some(c) = at {
                    if ui.button("Show").on_hover_text("Fly to what it changes").clicked() {
                        p.fly_to = Some(bevy::math::Vec2::new(c.x, c.y));
                    }
                }
            });
            if let Some(why) = refusal {
                ui.label(egui::RichText::new(format!("Will be refused: {}", why.trim_start_matches("Not implemented: "))).small().color(WARN));
            }
        });
    });
    if implement {
        super::toolbar::implement_active(p.sim, p.tools, p.selection, id, &name);
        *confirm = false;
    }
    if discard {
        p.sim.world.discard_project();
        p.tools.points.clear();
        p.tools.status = format!("{name} discarded.");
        *confirm = false;
    }
}

fn frame() -> egui::Frame {
    egui::Frame::new()
        .fill(egui::Color32::from_rgb(28, 22, 8))
        .stroke(egui::Stroke::new(2.0, GOLD))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(14, 8))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::roads::RoadKind;
    use nd_sim::Vec2;

    #[test]
    fn the_summary_counts_what_the_project_ends_up_changing() {
        let mut p = Project::default();
        let road = |x: f32| Gesture::Road { points: vec![Vec2::new(x, 0.0), Vec2::new(x, 100.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() };
        p.push(GestureId(1), Some(road(0.0)));
        p.push(GestureId(1), Some(road(5.0)));
        p.push(GestureId(2), Some(road(50.0)));
        p.push(GestureId(3), None);
        assert_eq!(summary(&p), "2 roads, 1 piece taken out");
        assert!(summary(&Project::default()) == "changes");
    }
}
