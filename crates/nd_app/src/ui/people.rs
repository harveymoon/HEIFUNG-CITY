//! The People window: who lives here, by class and housing tier, who was
//! pushed out, the income-inequality graph, and the favourites list.

use bevy::prelude::*;
use bevy_egui::egui;
use nd_sim::{CitizenId, Tier, World};

use super::tools::warn_if;
use super::{hint, section, UiCtx, GOLD, WARN};
use crate::tools::Selection;

/// Colours for the three classes: rust for low, gold for middle, cyan for
/// high, so the histogram reads like the tier colours on the map.
fn tier_color(t: Tier) -> egui::Color32 {
    match t {
        Tier::Low => egui::Color32::from_rgb(210, 95, 70),
        Tier::Mid => GOLD,
        Tier::High => egui::Color32::from_rgb(110, 200, 220),
    }
}

pub fn window(ui: &mut egui::Ui, p: &mut UiCtx) {
    let e = &p.sim.world.economy;
    section(ui, "POPULATION");
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{}", p.sim.world.citizens.len())).strong().size(18.0));
        ui.label(egui::RichText::new(format!("people   {} waiting to move in   {} unhoused   {} unemployed", e.waiting_immigrants, e.unhoused, e.unemployed)).small());
    });
    class_row(ui, "class", e.class_counts);
    class_row(ui, "housing", e.tier_counts);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("rents").weak());
        for (i, t) in Tier::ALL.iter().enumerate() {
            ui.label(egui::RichText::new(format!("{:.0}", e.avg_rent[i])).color(tier_color(*t)));
        }
        hint(ui, "per day, by housing tier");
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("pushed out").weak());
        ui.label(warn_if(e.displaced_total > 0, format!("{} displaced   {} left town   {} priced out", e.displaced_total, e.emigrated_total, e.priced_out_total)).small());
    });

    section(ui, "INCOME INEQUALITY");
    inequality(ui, &p.sim.world);

    favourites(ui, p);
}

/// Three figures coloured by class, with their shares.
fn class_row(ui: &mut egui::Ui, label: &str, counts: [u32; 3]) {
    let total = counts.iter().sum::<u32>().max(1) as f32;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).weak());
        for (i, t) in Tier::ALL.iter().enumerate() {
            ui.label(egui::RichText::new(format!("{} {} ({:.0}%)", t.label(), counts[i], counts[i] as f32 / total * 100.0)).color(tier_color(*t)));
        }
    });
}

/// Histogram of citizens over daily income, 16 bins, bars coloured by the
/// class that income makes them; beside it a Lorenz curve with the Gini
/// index. Both are drawn with the painter: no plotting crate, and a few
/// hundred shapes a frame while the window is open.
fn inequality(ui: &mut egui::Ui, w: &World) {
    let mut incomes: Vec<f32> = w.citizens.values().map(|c| c.income).collect();
    if incomes.is_empty() {
        hint(ui, "Nobody lives here yet.");
        return;
    }
    incomes.sort_by(f32::total_cmp);
    let total: f32 = incomes.iter().sum();
    let n = incomes.len();
    let top_decile: f32 = incomes[n - (n / 10).max(1)..].iter().sum::<f32>() / total.max(1.0);
    let median = incomes[n / 2];
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("Gini {:.2}", w.economy.gini)).strong().color(if w.economy.gini > 0.4 { WARN } else { GOLD }));
        hint(ui, &format!("median {median:.0}/day · mean {:.0}/day · top 10% earn {:.0}% of all income", total / n as f32, top_decile * 100.0));
    });
    ui.horizontal_top(|ui| {
        let width = (ui.available_width() - 140.0).max(220.0);
        histogram(ui, &incomes, width);
        lorenz(ui, &incomes, total);
    });
    hint(ui, "people over daily income; the dashed lines are the class thresholds. Right: share of income held by the poorest share of people, against perfect equality.");
}

const BINS: usize = 16;

fn histogram(ui: &mut egui::Ui, sorted: &[f32], width: f32) {
    // The axis ends on a round number above the richest citizen so the
    // labels read cleanly and the bins do not jitter as incomes drift.
    let max_income = sorted.last().copied().unwrap_or(0.0);
    let axis_max = ((max_income / 50.0).ceil() * 50.0).max(50.0);
    let mut bins = [0u32; BINS];
    for &income in sorted {
        let b = ((income / axis_max) * BINS as f32) as usize;
        bins[b.min(BINS - 1)] += 1;
    }
    let peak = bins.iter().copied().max().unwrap_or(1).max(1);

    let (resp, painter) = ui.allocate_painter(egui::vec2(width, 150.0), egui::Sense::hover());
    let rect = resp.rect;
    let plot = egui::Rect::from_min_max(rect.min + egui::vec2(30.0, 6.0), rect.max - egui::vec2(6.0, 16.0));
    painter.rect_filled(plot, 2.0, egui::Color32::from_rgba_unmultiplied(20, 22, 34, 220));
    let axis = egui::Stroke::new(1.0, egui::Color32::from_gray(110));
    painter.line_segment([plot.left_bottom(), plot.right_bottom()], axis);
    painter.line_segment([plot.left_top(), plot.left_bottom()], axis);

    let bw = plot.width() / BINS as f32;
    let bin_width = axis_max / BINS as f32;
    let mut last_tier = None;
    for (i, &count) in bins.iter().enumerate() {
        let x0 = plot.left() + i as f32 * bw;
        let tier = Tier::for_income((i as f32 + 0.5) * bin_width);
        // A class boundary falls inside this bin: mark its left edge.
        if last_tier.is_some_and(|t| t != tier) {
            dashed_vline(&painter, x0, plot.y_range(), egui::Color32::from_gray(150));
        }
        last_tier = Some(tier);
        if count == 0 {
            continue;
        }
        let h = (plot.height() * count as f32 / peak as f32).max(1.0);
        let bar = egui::Rect::from_min_max(egui::pos2(x0 + 1.0, plot.bottom() - h), egui::pos2(x0 + bw - 1.0, plot.bottom()));
        painter.rect_filled(bar, 1.0, tier_color(tier));
    }

    let small = egui::FontId::proportional(10.0);
    let grey = egui::Color32::from_gray(170);
    painter.text(egui::pos2(plot.left() - 3.0, plot.top()), egui::Align2::RIGHT_TOP, format!("{peak}"), small.clone(), grey);
    painter.text(egui::pos2(plot.left() - 3.0, plot.bottom()), egui::Align2::RIGHT_BOTTOM, "0", small.clone(), grey);
    painter.text(egui::pos2(plot.left(), plot.bottom() + 2.0), egui::Align2::LEFT_TOP, "0", small.clone(), grey);
    painter.text(egui::pos2(plot.right(), plot.bottom() + 2.0), egui::Align2::RIGHT_TOP, format!("{axis_max:.0}/day"), small.clone(), grey);
    painter.text(egui::pos2(plot.center().x, plot.bottom() + 2.0), egui::Align2::CENTER_TOP, "daily income", small, grey);

    // Hovering a bar names its range and count.
    if let Some(pos) = resp.hover_pos().filter(|p| plot.contains(*p)) {
        let i = (((pos.x - plot.left()) / bw) as usize).min(BINS - 1);
        resp.on_hover_text(format!("{:.0} to {:.0} per day: {} {}", i as f32 * bin_width, (i + 1) as f32 * bin_width, bins[i], if bins[i] == 1 { "person" } else { "people" }));
    }
}

fn dashed_vline(painter: &egui::Painter, x: f32, y: egui::Rangef, color: egui::Color32) {
    let mut y0 = y.min;
    while y0 < y.max {
        let y1 = (y0 + 4.0).min(y.max);
        painter.line_segment([egui::pos2(x, y0), egui::pos2(x, y1)], egui::Stroke::new(1.0, color));
        y0 += 8.0;
    }
}

/// Lorenz curve: cumulative share of income against cumulative share of
/// people, poorest first, sampled at 48 points however many citizens there
/// are. The gap below the diagonal is the Gini index, drawn.
fn lorenz(ui: &mut egui::Ui, sorted: &[f32], total: f32) {
    let (resp, painter) = ui.allocate_painter(egui::vec2(130.0, 150.0), egui::Sense::hover());
    let rect = resp.rect;
    let plot = egui::Rect::from_min_max(rect.min + egui::vec2(6.0, 6.0), rect.max - egui::vec2(6.0, 26.0));
    painter.rect_filled(plot, 2.0, egui::Color32::from_rgba_unmultiplied(20, 22, 34, 220));
    painter.line_segment([plot.left_bottom(), plot.right_top()], egui::Stroke::new(1.0, egui::Color32::from_gray(110)));

    const SAMPLES: usize = 48;
    let n = sorted.len();
    let mut points = Vec::with_capacity(SAMPLES + 1);
    let mut acc = 0.0;
    let mut taken = 0;
    for k in 0..=SAMPLES {
        let upto = k * n / SAMPLES;
        while taken < upto {
            acc += sorted[taken];
            taken += 1;
        }
        let share = if total > 0.0 { acc / total } else { k as f32 / SAMPLES as f32 };
        points.push(egui::pos2(plot.left() + plot.width() * k as f32 / SAMPLES as f32, plot.bottom() - plot.height() * share));
    }
    painter.line(points, egui::Stroke::new(1.5, GOLD));
    let small = egui::FontId::proportional(10.0);
    let grey = egui::Color32::from_gray(170);
    painter.text(egui::pos2(plot.center().x, plot.bottom() + 2.0), egui::Align2::CENTER_TOP, "people, poorest first", small.clone(), grey);
    painter.text(egui::pos2(plot.center().x, plot.bottom() + 13.0), egui::Align2::CENTER_TOP, "↑ share of income", small, grey);
    resp.on_hover_text("Lorenz curve: the further it sags below the diagonal, the more unequal the city. Gini is twice the area between them.");
}

/// Starred people, with a button to select and one to fly the camera to
/// them. Anyone who left town drops off the list.
fn favourites(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    p.tools.favorites.retain(|id| w.citizens.contains_key(*id));
    section(ui, "FAVOURITES");
    if p.tools.favorites.is_empty() {
        hint(ui, "Star someone in their person pane (Inspect tool, click a citizen) to keep them here.");
        return;
    }
    let mut pick: Option<CitizenId> = None;
    let mut jump: Option<nd_sim::Vec2> = None;
    let mut drop: Option<CitizenId> = None;
    for &id in &p.tools.favorites {
        let Some(c) = w.citizens.get(id) else { continue };
        ui.horizontal(|ui| {
            let sel = *p.selection == Selection::Citizen(id);
            if ui.selectable_label(sel, &c.name).clicked() {
                pick = Some(id);
            }
            hint(ui, c.activity.label());
            if ui.small_button("go").on_hover_text("Select and fly the camera to them").clicked() {
                pick = Some(id);
                jump = Some(c.pos);
            }
            if ui.small_button("★").on_hover_text("Remove from favourites").clicked() {
                drop = Some(id);
            }
        });
    }
    if let Some(id) = pick {
        *p.selection = Selection::Citizen(id);
    }
    if let Some(id) = drop {
        p.tools.favorites.retain(|f| *f != id);
    }
    if let Some(pos) = jump {
        p.fly_to = Some(Vec2::new(pos.x, pos.y));
    }
}
