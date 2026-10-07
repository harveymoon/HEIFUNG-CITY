//! [inspector] Head-and-shoulders portraits of citizens, painted from their
//! look (`nd_sim::looks`): skin, hair (style and colour, greying with age,
//! dye), the outfit in their two favourite colours, a child's rounder face.
//! The inspector's "people here" grid, a car's driver and occupants, and
//! a bus's riders use them; clicking one selects the person.
//!
//! They are a dozen egui shapes each, painted every frame like the rest of
//! the HUD: cheaper than caching a texture per citizen, and always the
//! person's current look (grey hair arrives on its own).

use bevy_egui::egui::{self, Color32, Pos2, Rect, Stroke};
use nd_sim::looks::{skin_srgb, HairStyle, Outfit, COLORS};
use nd_sim::{Citizen, CitizenId, World};

use crate::tools::Selection;

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Paint a citizen's bust into `rect` (square works best).
pub fn paint(painter: &egui::Painter, rect: Rect, c: &Citizen) {
    let a = c.appearance();
    let fav = a.fav.map(|i| rgb(COLORS[(i as usize).min(COLORS.len() - 1)].srgb));
    let skin = rgb(skin_srgb(a.genes.skin));
    let hair = rgb(a.shown_hair(c.age).srgb());
    let s = rect.width().min(rect.height());
    let cx = rect.center().x;
    // A dark disc of their main colour behind them, so a row of faces
    // reads as a row of people even where outfits match the panel.
    painter.rect_filled(rect, 4.0, fav[0].gamma_multiply(0.22));
    let child = c.age < nd_sim::citizens::ADULT_AGE;
    let head_r = s * if child { 0.2 } else { 0.17 };
    let head = Pos2::new(cx, rect.top() + s * if child { 0.44 } else { 0.4 });
    let at = |x: f32, y: f32| Pos2::new(cx + x * s, rect.top() + y * s);
    // Shoulders: the outfit's top in the main colour, collar or trim in the accent.
    let wide = match a.build {
        nd_sim::looks::Build::Slim => 0.34,
        nd_sim::looks::Build::Average => 0.38,
        nd_sim::looks::Build::Broad => 0.43,
    } * if child { 0.8 } else { 1.0 };
    let shoulders = Rect::from_min_max(at(-wide, 0.72), at(wide, 1.0));
    painter.rect_filled(shoulders.intersect(rect), egui::CornerRadius { nw: (s * 0.18) as u8, ne: (s * 0.18) as u8, sw: 3, se: 3 }, fav[0]);
    // Neck.
    painter.rect_filled(Rect::from_min_max(at(-0.06, 0.55), at(0.06, 0.76)), 1.0, skin);
    match a.outfit {
        Outfit::Suit => {
            // Shirt and lapels: a V of the accent, a dark tie.
            painter.add(egui::Shape::convex_polygon(vec![at(-0.1, 0.72), at(0.1, 0.72), at(0.0, 0.92)], fav[1], Stroke::NONE));
            painter.line_segment([at(0.0, 0.75), at(0.0, 0.9)], Stroke::new(s * 0.03, Color32::from_gray(25)));
        }
        Outfit::Hoodie => {
            // The hood lies round the neck.
            painter.rect_filled(Rect::from_min_max(at(-0.16, 0.68), at(0.16, 0.78)), 4.0, fav[1]);
        }
        Outfit::WorkJacket => {
            painter.rect_filled(Rect::from_min_max(at(-wide, 0.86), at(wide, 0.9)), 0.0, Color32::from_rgb(230, 200, 60));
            painter.line_segment([at(0.0, 0.76), at(0.0, 1.0)], Stroke::new(1.0, fav[1]));
        }
        Outfit::Coat => {
            painter.add(egui::Shape::convex_polygon(vec![at(-0.13, 0.72), at(0.0, 0.86), at(-0.03, 0.72)], fav[1], Stroke::NONE));
            painter.add(egui::Shape::convex_polygon(vec![at(0.03, 0.72), at(0.0, 0.86), at(0.13, 0.72)], fav[1], Stroke::NONE));
        }
        Outfit::Dress => {
            painter.rect_filled(Rect::from_min_max(at(-0.12, 0.72), at(0.12, 0.76)), 2.0, skin);
            painter.rect_filled(Rect::from_min_max(at(-wide * 0.9, 0.9), at(wide * 0.9, 0.93)), 0.0, fav[1]);
        }
        Outfit::Tee => {
            painter.rect_filled(Rect::from_min_max(at(-0.1, 0.72), at(0.1, 0.75)), 1.0, fav[1]);
        }
    }
    // Hair behind the head (long hair, an afro, a bun).
    match a.style {
        HairStyle::Long => {
            painter.rect_filled(Rect::from_center_size(head + egui::vec2(0.0, head_r * 0.6), egui::vec2(head_r * 2.3, head_r * 2.4)), head_r * 0.6, hair);
        }
        HairStyle::Afro => {
            painter.circle_filled(head - egui::vec2(0.0, head_r * 0.25), head_r * 1.45, hair);
        }
        HairStyle::Bun => {
            painter.circle_filled(head - egui::vec2(0.0, head_r * 1.15), head_r * 0.45, hair);
        }
        _ => {}
    }
    // The face.
    painter.circle_filled(head, head_r, skin);
    // Hair on top.
    let cap = |painter: &egui::Painter, depth: f32, width: f32| {
        let top = Rect::from_min_max(head - egui::vec2(head_r * width, head_r * 1.08), head + egui::vec2(head_r * width, -head_r * (1.0 - depth)));
        painter.rect_filled(top, egui::CornerRadius { nw: (head_r * 0.9) as u8, ne: (head_r * 0.9) as u8, sw: 0, se: 0 }, hair);
    };
    match a.style {
        HairStyle::Bald => {}
        HairStyle::Buzz => cap(painter, 0.45, 0.98),
        HairStyle::Short => cap(painter, 0.7, 1.05),
        HairStyle::Long | HairStyle::Bun => cap(painter, 0.75, 1.05),
        HairStyle::Afro => cap(painter, 0.6, 1.1),
        HairStyle::Mohawk => {
            painter.rect_filled(Rect::from_center_size(head - egui::vec2(0.0, head_r * 0.95), egui::vec2(head_r * 0.35, head_r * 0.9)), 1.0, hair);
        }
    }
    // Eyes: two dark dots, a little apart for a child's rounder face.
    let eye = head_r * 0.12;
    let ey = head.y + head_r * 0.08;
    let ex = head_r * if child { 0.4 } else { 0.36 };
    painter.circle_filled(Pos2::new(cx - ex, ey), eye.max(1.0), Color32::from_gray(20));
    painter.circle_filled(Pos2::new(cx + ex, ey), eye.max(1.0), Color32::from_gray(20));
}

/// A portrait as a widget: clickable, with the name and what they are
/// doing on hover.
pub fn button(ui: &mut egui::Ui, c: &Citizen, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), rect, c);
        if resp.hovered() {
            ui.painter().rect_stroke(rect, 4.0, Stroke::new(1.5, super::GOLD), egui::StrokeKind::Inside);
        }
    }
    resp.on_hover_text(format!("{}, {}: {}. Click to select.", c.name, c.age, c.activity.label()))
}

/// A wrapping grid of portraits with the first name under each; clicking
/// one selects that person (the person pane opens on them). Shows at most
/// `max` and says how many more there are.
pub fn grid(ui: &mut egui::Ui, w: &World, ids: &[CitizenId], max: usize, selection: &mut Selection) {
    const CELL: f32 = 56.0;
    let people: Vec<(CitizenId, &Citizen)> = ids.iter().filter_map(|&id| w.citizens.get(id).map(|c| (id, c))).collect();
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 6.0);
        for &(id, c) in people.iter().take(max) {
            ui.allocate_ui_with_layout(egui::vec2(CELL, CELL + 16.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                ui.set_width(CELL);
                if button(ui, c, CELL - 12.0).clicked() {
                    picked = Some(id);
                }
                let first = c.name.split_whitespace().next().unwrap_or(&c.name);
                let label = ui.add(egui::Label::new(egui::RichText::new(first).small()).truncate().sense(egui::Sense::click()));
                if label.on_hover_text(&c.name).clicked() {
                    picked = Some(id);
                }
            });
        }
    });
    if people.len() > max {
        super::hint(ui, &format!("and {} more", people.len() - max));
    }
    if let Some(id) = picked {
        *selection = Selection::Citizen(id);
    }
}
