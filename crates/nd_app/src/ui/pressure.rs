//! [economy] Business pressure in the HUD: City Data's Economy, Overview: "What the
//! city's businesses want" (every pressure with its strength and why, from
//! `nd_sim::pressure`), and the inspector's "why it's here / how it's
//! doing" for a business (`World::business_story`).

use bevy_egui::egui::{self, Color32};
use nd_sim::pressure::{Pressure, PressureKind};
use nd_sim::{BuildingId, World};

use super::pane::heading;
use super::fmt::capitalise;
use super::theme::GOOD;
use super::{hint, GOLD, IVORY, WARN};


/// A pressure's bar colour: hot when it wants more, grey when met.
fn heat(p: &Pressure) -> Color32 {
    if p.held.is_some() && p.wants_more() {
        Color32::from_rgb(200, 160, 90)
    } else if p.wants_more() {
        WARN
    } else if p.strength > 0.0 {
        GOLD
    } else {
        Color32::from_gray(110)
    }
}

/// The readings, strongest first (ties in the enum's order).
pub fn sorted(w: &World) -> Vec<&Pressure> {
    let mut v: Vec<&Pressure> = w.pressure.readings.iter().collect();
    v.sort_by(|a, b| b.strength.total_cmp(&a.strength).then(b.gap.total_cmp(&a.gap)).then(a.kind.cmp(&b.kind)));
    v
}

fn bar(ui: &mut egui::Ui, value: f32, col: Color32) {
    let (resp, painter) = ui.allocate_painter(egui::vec2(70.0, 10.0), egui::Sense::hover());
    let r = resp.rect;
    painter.rect_filled(r, 2.0, Color32::from_gray(35));
    let w = r.width() * value.clamp(0.0, 1.0);
    if w > 0.0 {
        painter.rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(w, r.height())), 2.0, col);
    }
}

fn row(ui: &mut egui::Ui, p: &Pressure, full: bool) {
    ui.horizontal(|ui| {
        ui.add_sized(egui::vec2(130.0, 14.0), egui::Label::new(egui::RichText::new(capitalise(p.kind.label())).small().color(IVORY)));
        bar(ui, p.strength, heat(p));
        let status = if p.wants_more() {
            match &p.held {
                Some(_) => "wanted, held".to_string(),
                None => "opening one".to_string(),
            }
        } else if p.wanted < 0.05 {
            "no call".to_string()
        } else if p.gap <= -0.5 {
            "more than enough".to_string()
        } else {
            "met".to_string()
        };
        let have = if p.kind == PressureKind::Marina { format!("{} built", p.have) } else { format!("{} in town", p.have) };
        ui.label(egui::RichText::new(format!("{have}, {:.1} wanted  ·  {status}", p.wanted)).small());
        if p.struggling > 0 {
            ui.label(egui::RichText::new(format!("{} struggling", p.struggling)).small().color(WARN));
        }
    });
    if full || p.wants_more() {
        ui.label(egui::RichText::new(format!("    {}", p.why)).small().weak());
        if let Some(h) = &p.held {
            ui.label(egui::RichText::new(format!("    held: {h}")).small().color(Color32::from_rgb(200, 160, 90)));
        }
    }
}

/// Economy, Overview: every pressure, why, and the scene around it.
pub fn wanted(ui: &mut egui::Ui, w: &World) {
    heading(ui, "What the city's businesses want");
    hint(ui, "Each trade is opened by a cause in the city. Strength is the unmet need: a full bar is one business more than stands. One opens when a pressure holds two days and a lot suits it; one that cannot pay its way closes like any other, and its kind waits a while.");
    if w.pressure.readings.is_empty() {
        hint(ui, "No reading yet: the first comes at midnight.");
        return;
    }
    for p in sorted(w) {
        row(ui, p, true);
    }
    let t = &w.society.trash.stats;
    let companies = w.trash_companies().len();
    if companies > 0 || t.local_clients > 0 {
        ui.label(egui::RichText::new(format!("Trash: {companies} local compan{} serve {} buildings at {:.2} a household a day (the hauler from out of town charges {:.2}).", if companies == 1 { "y" } else { "ies" }, t.local_clients, t.local_fee, nd_sim::trash::PRIVATE_FEE_HOUSEHOLD)).small());
    }
    let s = &w.scene;
    let sig = &w.pressure.signals;
    let scene = if sig.art_scene() { "an art scene" } else { "no art scene yet" };
    ui.label(egui::RichText::new(format!("Art: {:.0} artists ({:.0} getting by), {:.0} rich adults: {scene}. {} openings held, {:.0} given by patrons, {} pieces sold.", sig.artists, sig.artists_mid, sig.rich_adults, s.openings_total, s.donations_total, s.art_sold_total)).small());
    if sig.marinas_small + sig.marinas_large + sig.fishing_small + sig.fishing_large > 0.0 {
        ui.label(egui::RichText::new(format!("Waterfront: {:.0} marinas, {:.0} fishing ports, {} boats out now, {:.0} in berth fees last night.", sig.marinas_small + sig.marinas_large, sig.fishing_small + sig.fishing_large, s.boats_out, s.berth_fees_today)).small());
    }
    let opened: Vec<String> = w.pressure.opened_total.iter().map(|(k, n)| format!("{} {}", n, k.replace('_', " "))).collect();
    if !opened.is_empty() {
        hint(ui, &format!("Opened by pressure so far: {}.", opened.join(", ")));
    }
}

/// [inspector] Why a business is here and how it is doing.
pub fn business_story(ui: &mut egui::Ui, w: &World, id: BuildingId) {
    let Some(s) = w.business_story(id) else { return };
    ui.label(egui::RichText::new("Why it's here").small().strong().color(GOLD));
    ui.label(egui::RichText::new(&s.why).small());
    ui.label(egui::RichText::new("How it's doing").small().strong().color(GOLD));
    ui.label(egui::RichText::new(&s.doing).small().color(if s.struggling { WARN } else { IVORY }));
    if s.wants_expand {
        ui.label(egui::RichText::new("WANTS TO EXPAND").small().strong().color(GOOD));
    }
}
