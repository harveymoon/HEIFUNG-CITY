//! The inspector on a pavement bot.

use super::*;

/// [camera] A pavement delivery robot: its name and code, the shop that
/// owns it (in the fleet's colour), what it carries, where it is going and
/// how far, and the household that ordered. Owner and destination select.
pub(super) fn bot(ui: &mut egui::Ui, w: &nd_sim::World, selection: &mut Selection, id: nd_sim::BotId) {
    let Some(b) = w.bots.bots.get(id) else {
        *selection = Selection::None;
        return;
    };
    let [r, g, bl] = nd_sim::bots::fleet_colour(b.shop);
    let fleet = egui::Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (bl * 255.0) as u8);
    ui.label(egui::RichText::new(nd_sim::bots::bot_name(w, id)).font(crate::ui::theme::lead()).color(IVORY).strong());
    let size = w.bots.fleet_of(b.shop).len();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        tag(ui, "Delivery bot", GOLD);
        tag(ui, &nd_sim::bots::bot_code(id), egui::Color32::from_gray(150));
        tag(ui, &format!("fleet of {size}"), fleet);
    });
    let name = |x: nd_sim::BuildingId| w.buildings.get(x).map_or("a building that is gone".to_string(), |b| b.name.clone());
    section(ui, "OWNER");
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, fleet);
        let kind = w.buildings.get(b.shop).and_then(|s| s.business.as_ref()).map(|z| capitalise(z.kind.label()));
        if ui.link(egui::RichText::new(name(b.shop)).color(fleet)).on_hover_text("Select the shop").clicked() {
            *selection = Selection::Building(b.shop);
        }
        if let Some(k) = kind {
            hint(ui, &k);
        }
    });
    section(ui, "TRIP");
    match b.cargo() {
        Some((res, n)) => ui.label(format!("Carrying {n:.0} {} to the customer's door", res.label())),
        None => ui.label("Delivered; trundling back to the shop, hold empty"),
    };
    ui.horizontal(|ui| {
        ui.label("Going to");
        let to = b.destination();
        if ui.link(name(to)).on_hover_text("Select it").clicked() {
            *selection = Selection::Building(to);
        }
    });
    let m = b.metres_left();
    ui.label(format!("{m:.0} m of pavement left  ·  about {:.0} min  ·  out {} min", (m / nd_sim::bots::BOT_MPS / 60.0).ceil(), b.age));
    if !b.returning {
        ui.horizontal(|ui| {
            hint(ui, "ordered by a household at");
            if ui.link(egui::RichText::new(name(b.home)).small()).clicked() {
                *selection = Selection::Building(b.home);
            }
        });
    }
    hint(ui, "Route: the line on the map, in the fleet's colour.");
}
