//! [ferry-routes] The inspector's ferry cards: a terminal's (or shipping
//! port's) routes, one row each with its colour, where it goes (click to
//! fly there), the boats on it, how often it runs, the next boat out and
//! the riders today; and the card for a selected ferry or freighter.

use bevy::math::Vec2;
use bevy_egui::egui;
use nd_sim::world::ferry_routes::{LineEnd, LineKind};
use nd_sim::{BuildingId, World};

use super::{hint, neon, section, WARN};
use crate::tools::Selection;

const FLY_KEY: &str = "ferry_route_fly";

/// Where a click on a route asked the camera to go, taken once (the
/// inspector window hands it to the camera).
pub fn take_fly(ctx: &egui::Context) -> Option<Vec2> {
    ctx.data_mut(|d| d.remove_temp::<[f32; 2]>(egui::Id::new(FLY_KEY))).map(|[x, y]| Vec2::new(x, y))
}

fn hhmm(tick: u64) -> String {
    format!("{:02}:{:02}", (tick % 1440) / 60, tick % 60)
}

/// The FERRIES / SHIPPING LANE section of a dock's inspector.
pub fn dock_routes(ui: &mut egui::Ui, w: &World, id: BuildingId) {
    let Some(kind) = w.ports.kind_of(id).filter(|_| crate::ferry_select::is_route_dock(w, id)) else { return };
    let ferry = kind == nd_sim::PortKind::FerryTerminal;
    section(ui, if ferry { "FERRY ROUTES" } else { "SHIPPING LANE" });
    if ferry {
        let towns = w.ferry_towns(id);
        let waiting = w.ports.boarding.get(&id).copied().unwrap_or(0);
        if towns.is_empty() {
            hint(ui, "its water does not reach the edge of the map, so no boats run off it");
        } else {
            hint(ui, &format!("{waiting} waiting to board for the towns off the map"));
        }
    }
    if w.ports.port_of(id).is_some_and(|p| !p.works()) {
        let why = if w.ports.port_of(id).is_some_and(|p| !p.road_connected) { "join its street stub to the city's roads" } else { "no ship can reach it from off the map" };
        ui.label(egui::RichText::new(format!("not running: {why}")).color(WARN));
    }
    let (_, net) = crate::ferry_select::net(w);
    let infos = w.line_infos(&net, id);
    if infos.is_empty() {
        hint(ui, if ferry { "no routes: build another terminal on connected water, or one on water that reaches the edge of the map" } else { "no lane: the water here does not reach the edge of the map" });
        return;
    }
    let now = w.clock.tick;
    for i in &infos {
        let l = &net.lines[i.line];
        let col = crate::ferry_select::line_color32(l);
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().rect_filled(r.shrink(2.0), 3.0, col);
            let to = match i.end {
                LineEnd::Town(_) => format!("{} (off the map)", i.end_name),
                LineEnd::Elsewhere => "to the open sea".to_string(),
                LineEnd::Terminal(_) => i.end_name.clone(),
            };
            let link = ui.add(egui::Button::new(egui::RichText::new(format!("to {to}")).color(col).strong()).frame(false)).on_hover_text("Fly the camera to the far end of this route");
            if link.clicked() {
                let at = [i.end_at.x, i.end_at.y];
                ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(FLY_KEY), at));
            }
            hint(ui, &format!("{:.1} km", i.length / 1000.0));
        });
        let boats = i.ships.len();
        let what = match i.kind {
            LineKind::Ferry => if boats == 1 { "ferry" } else { "ferries" },
            LineKind::Freight => if boats == 1 { "freighter" } else { "freighters" },
        };
        let mut line = format!("{boats} {what} on it");
        if !i.running {
            line += "  ·  not running";
        } else {
            if let Some(h) = i.headway {
                line += &format!("  ·  every ~{h} min");
            }
            match i.next_departure {
                Some(t) if t <= now + 1 => line += "  ·  next boat due now",
                Some(t) if t / 1440 != now / 1440 => line += &format!("  ·  next boat tomorrow {}", hhmm(t)),
                Some(t) => line += &format!("  ·  next boat {}", hhmm(t)),
                None if i.kind == LineKind::Ferry => line += "  ·  no boat from here soon",
                None => {}
            }
        }
        if i.kind == LineKind::Ferry {
            line += &format!("  ·  {} passengers today", i.passengers_today);
        }
        ui.label(egui::RichText::new(line).small());
    }
    hint(ui, "the routes are drawn on the map; others in the city are dimmed");
}

/// The card for a selected ferry or freighter.
pub fn ship_card(ui: &mut egui::Ui, w: &World, selection: &mut Selection, id: nd_sim::ports::ShipId) {
    let Some(s) = w.ports.ships.get(id) else {
        *selection = Selection::None;
        return;
    };
    neon(ui, &crate::ferry_select::ship_title(w, id));
    ui.label(crate::ferry_select::ship_doing(w, id));
    match s.kind {
        nd_sim::ShipKind::Freighter => {
            ui.label(format!("{} load{} aboard  ·  {:.0} m long", s.cargo.len(), if s.cargo.len() == 1 { "" } else { "s" }, s.length()));
        }
        _ => {
            ui.label(format!("{} passenger{} aboard of {}  ·  {:.0} m long", s.passengers, if s.passengers == 1 { "" } else { "s" }, nd_sim::ports::FERRY_CAPACITY, s.length()));
        }
    }
    let (_, net) = crate::ferry_select::net(w);
    if let Some(l) = net.of_ship(s).map(|i| &net.lines[i]) {
        let col = crate::ferry_select::line_color32(l);
        section(ui, "ROUTE");
        ui.label(egui::RichText::new(format!("{} - {}", w.buildings.get(l.from).map_or("?", |b| b.name.as_str()), w.line_end_name(l.from, l.to))).color(col).strong());
    }
    if let Some(b) = w.buildings.get(s.port) {
        if ui.small_button(format!("Select {}", b.name)).on_hover_text("Its home terminal, with all its routes").clicked() {
            *selection = Selection::Building(s.port);
        }
    }
}
