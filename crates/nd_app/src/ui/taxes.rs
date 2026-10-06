//! [tax] [tolls] The Taxes window: a rate per land use and sub-type, set
//! with a slider, beside the number of buildings under it and a bar of what
//! it brought in at the last collection (with a tick at its running daily
//! average; bars are scaled within their land use); a stacked bar of the
//! take by land use; the import duty; and every toll booth with its price,
//! traffic and takings.
//!
//! Incomes and takings are taxed once a day at midnight, so the bars show
//! the last collection; they stay empty on a city's first day.

use bevy::prelude::Vec2;
use bevy_egui::egui;
use nd_sim::tax::TaxCategory;
use nd_sim::LandUse;

use super::{hint, section, zone_color32, UiCtx, GOLD, WARN};

const LESS: egui::Color32 = egui::Color32::from_rgb(110, 200, 140);
const AVG_TICK: egui::Color32 = egui::Color32::from_rgb(235, 225, 200);

pub fn window(ui: &mut egui::Ui, p: &mut UiCtx) {
    let rows = p.sim.world.tax_breakdown();
    let palette = &p.assets.palette;
    let groups = [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial];
    let total_today: f32 = rows.iter().map(|r| r.today).sum();
    let (duty_today, duty_yesterday, duty) = {
        let t = &p.sim.world.economy.taxes;
        (t.duty_today, t.duty_yesterday, t.import_duty)
    };

    section(ui, "LAST COLLECTION BY LAND USE");
    // One stacked bar: each land use's share of the take, in its zone colour.
    let (resp, painter) = ui.allocate_painter(egui::vec2(ui.available_width().min(360.0), 16.0), egui::Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 2.0, egui::Color32::from_gray(30));
    let mut x = rect.left();
    let mut tip = Vec::new();
    for lu in groups {
        let sum: f32 = rows.iter().filter(|r| r.category.land_use() == lu).map(|r| r.today).sum();
        tip.push(format!("{}: {:.0}", lu.label(), sum));
        if total_today <= 0.0 || sum <= 0.0 {
            continue;
        }
        let w = rect.width() * sum / total_today;
        painter.rect_filled(egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(x + w, rect.bottom())), 0.0, zone_color32(palette.zone_color(lu)).gamma_multiply(0.85));
        x += w;
    }
    resp.on_hover_text(tip.join("\n"));
    let tolls = p.sim.world.economy.ledger.today_of(nd_sim::economy::Flow::Tolls);
    hint(ui, &format!("{total_today:.0} in taxes at the last midnight · import duty {duty_today:.0} today ({duty_yesterday:.0} yesterday) · tolls {tolls:.0} today"));
    ui.horizontal(|ui| {
        if ui.button("Reset to standard rates").on_hover_text("12% of incomes, 5% of business takings, no import duty").clicked() {
            p.sim.world.economy.taxes.reset();
            p.sim.world.economy.recompute();
        }
        if p.sim.world.economy.taxes.is_standard() && duty == 0.0 {
            hint(ui, "standard rates");
        }
    });

    for lu in groups {
        let col = zone_color32(palette.zone_color(lu));
        let mine: Vec<_> = rows.iter().filter(|r| r.category.land_use() == lu).collect();
        // Each group's longest bar sets its scale: incomes dwarf the tax on
        // takings, and the stacked bar above already compares the groups.
        let scale = mine.iter().map(|r| r.today.max(r.recent)).fold(1.0, f32::max);
        let buildings: u32 = mine.iter().map(|r| r.buildings).sum();
        let take: f32 = mine.iter().map(|r| r.today).sum();
        let what = if lu == LandUse::Residential { "income tax by home tier" } else { "tax on takings" };
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(lu.label().to_uppercase()).small().strong().color(col));
            ui.label(egui::RichText::new(format!("{what} · {buildings} buildings · {take:.0}")).small().weak());
        });
        egui::Grid::new(("tax_lines", lu.index())).num_columns(4).spacing(egui::vec2(8.0, 3.0)).show(ui, |ui| {
            for r in mine {
                let c = r.category;
                ui.label(egui::RichText::new(c.label()).small());
                ui.label(egui::RichText::new(format!("{} bldg", r.buildings)).small().weak()).on_hover_text(format!(
                    "{} {} · {} {}",
                    r.capacity,
                    if c.is_residential() { "homes" } else { "jobs" },
                    r.people,
                    if c.is_residential() { "residents" } else { "workers" }
                ));
                let mut pct = r.rate * 100.0;
                let max = c.max_rate() * 100.0;
                ui.spacing_mut().slider_width = 110.0;
                let resp = ui.add(egui::Slider::new(&mut pct, 0.0..=max).step_by(0.5).suffix("%").fixed_decimals(1)).on_hover_text(effect_text(c, r.default_rate, r.pull));
                if resp.changed() {
                    p.sim.world.set_tax_rate(c, pct / 100.0);
                }
                take_bar(ui, r.today, r.recent, scale, r.pull);
                ui.end_row();
            }
        });
    }

    section(ui, "IMPORT DUTY");
    ui.horizontal(|ui| {
        let mut pct = duty * 100.0;
        ui.spacing_mut().slider_width = 160.0;
        if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).step_by(1.0).suffix("%")).on_hover_text("Added to the price of everything bought in from outside, and paid to the treasury. Industry that makes what the town imports is drawn harder; businesses living on imports earn less.").changed() {
            p.sim.world.set_import_duty(pct / 100.0);
        }
        ui.label(egui::RichText::new(format!("{duty_today:.0} today · {:.0} imported", p.sim.world.economy.market.imports_today)).small().weak());
    });

    section(ui, "TOLL BOOTHS");
    if p.sim.world.tolls.booths.is_empty() {
        hint(ui, "None yet. Buildings > Transit > Toll booth or Toll plaza, then click a straight stretch of road.");
        return;
    }
    let t = &p.sim.world.tolls;
    hint(ui, &format!("{} paid today ({} lorries at double), {} free (buses, emergencies) · {:.0} taken · upkeep {:.0}/day", t.paid_today, t.trucks_today, t.free_today, t.revenue_today(), t.upkeep_per_day()));
    let booths: Vec<(u32, String, &'static str, f32, u32, f32, f32, Vec2)> = t.booths.iter().map(|b| (b.id, b.name.clone(), b.size.label(), b.price, b.cars_today, b.revenue_today, b.revenue_yesterday, Vec2::new(b.pos.x, b.pos.y))).collect();
    let mut remove = None;
    egui::Grid::new("toll_booths").num_columns(4).spacing(egui::vec2(8.0, 3.0)).show(ui, |ui| {
        for (id, name, size, price, cars, rev, rev_y, pos) in booths {
            ui.label(egui::RichText::new(&name).small()).on_hover_text(format!("{size} · {rev_y:.0} yesterday"));
            let mut pr = price;
            ui.spacing_mut().slider_width = 90.0;
            if ui.add(egui::Slider::new(&mut pr, 0.0..=20.0).step_by(0.5).prefix("toll ")).changed() {
                let _ = p.sim.world.set_toll_price(id, pr);
            }
            ui.label(egui::RichText::new(format!("{cars} cars · {rev:.0}")).small().weak());
            ui.horizontal(|ui| {
                if ui.small_button("go").clicked() {
                    p.fly_to = Some(pos);
                }
                if ui.small_button(egui::RichText::new("remove").color(WARN)).clicked() {
                    remove = Some(id);
                }
            });
            ui.end_row();
        }
    });
    if let Some(id) = remove {
        let _ = p.sim.world.remove_toll(id);
    }
}

/// What the line's rate does, for the slider's tooltip.
fn effect_text(c: TaxCategory, standard: f32, pull: f32) -> String {
    let who = if c.is_residential() { "Households here pay the difference from the standard out of their money every day; newcomers and new homes of this tier follow the pull." } else { "Businesses here pay the difference from the standard out of their takings; it shows in whether more of them are built." };
    format!("standard {:.0}% · pull {:.0}% of normal\n{who}", standard * 100.0, pull * 100.0)
}

/// The last collection as a bar (green when the line is let off, gold at
/// the standard, red when taxed over it), its running average as a tick.
fn take_bar(ui: &mut egui::Ui, today: f32, recent: f32, scale: f32, pull: f32) {
    ui.horizontal(|ui| {
        let (resp, painter) = ui.allocate_painter(egui::vec2(90.0, 12.0), egui::Sense::hover());
        let rect = resp.rect;
        painter.rect_filled(rect, 1.0, egui::Color32::from_gray(28));
        let col = if pull > 1.001 { LESS } else if pull < 0.999 { WARN } else { GOLD };
        let w = (today / scale).clamp(0.0, 1.0) * rect.width();
        if w > 0.0 {
            painter.rect_filled(egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + w.max(1.0), rect.bottom())), 1.0, col.gamma_multiply(0.85));
        }
        if recent > 0.0 {
            let x = rect.left() + (recent / scale).clamp(0.0, 1.0) * rect.width();
            painter.vline(x, rect.y_range(), egui::Stroke::new(1.5, AVG_TICK));
        }
        resp.on_hover_text(format!("last collection {today:.0} · average {recent:.0}/day"));
        ui.label(egui::RichText::new(format!("{today:.0}")).small());
    });
}
