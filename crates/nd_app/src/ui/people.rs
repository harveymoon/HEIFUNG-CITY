//! City Data's People tab: Population (who lives here over time, by class
//! and housing tier, who was pushed out), Income and inequality (the classes
//! over time, the Gini index, the income histogram and the Lorenz curve) and
//! Favourites. Complaints and concerns are `ui/concerns.rs`, Social is
//! `ui/social.rs`.

use bevy::prelude::*;
use bevy_egui::egui;
use nd_sim::{CitizenId, Tier, World};

use super::tools::warn_if;
use super::pane::{self, heading, row, Series};
use super::{hint, UiCtx, GOLD, WARN};
use bevy_egui::egui::Color32;
use crate::tools::Selection;

/// Colours for the three classes: rust for low, gold for middle, cyan for
/// high, so the histogram reads like the tier colours on the map.
pub(super) fn tier_color(t: Tier) -> egui::Color32 {
    match t {
        Tier::Low => egui::Color32::from_rgb(210, 95, 70),
        Tier::Mid => GOLD,
        Tier::High => egui::Color32::from_rgb(110, 200, 220),
    }
}

/// People, Population: the headline figures, population and what it is
/// made of over the range (hourly people for 72 h), then class, housing,
/// rents and who was pushed out.
pub fn population(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::data_span(ui.ctx());
    let w = &p.sim.world;
    let e = &w.economy;
    let h = &e.history;
    let days = pane::days(h, span);
    let first = days.first().map(|d| d.population).unwrap_or(0) as f32;
    let now = w.citizens.len() as f32;
    pane::cards(ui, |ui| {
        pane::kpi(ui, "people", egui::RichText::new(format!("{now:.0}")).color(GOLD));
        pane::kpi(ui, &format!("change over {} d", days.len()), warn_if(now < first, format!("{:+.0}", now - first)));
        pane::kpi(ui, "growth yesterday", egui::RichText::new(format!("{:+.1}%", e.growth_per_day * 100.0)));
        pane::kpi(ui, "households", egui::RichText::new(format!("{}", e.households)));
        pane::kpi(ui, "unemployment", warn_if(e.unemployment_share() > 0.08, format!("{:.0}%", e.unemployment_share() * 100.0)));
        pane::kpi(ui, "unhoused", warn_if(e.unhoused > 0, format!("{}", e.unhoused)));
    });
    heading(ui, "Population over time");
    if span == pane::Span::Hours {
        let hours = pane::hours(h);
        pane::line_chart(ui, &pane::hour_labels(&hours), &[Series::new("people", GOLD, pane::hcol(&hours, |r| r.population as f32))], 160.0, &|x| format!("{x:.0}"));
    } else {
        let labels = pane::day_labels(&days);
        let series = [
            Series::new("people", GOLD, pane::col(&days, |d| d.population as f32)),
            Series::new("households", Color32::from_rgb(110, 200, 220), pane::col(&days, |d| d.households as f32)),
            Series::new("employed", Color32::from_rgb(110, 200, 140), pane::col(&days, |d| d.employed as f32)),
            Series::new("unemployed", WARN, pane::col(&days, |d| d.unemployed as f32)),
            Series::new("unhoused", Color32::from_rgb(200, 120, 230), pane::col(&days, |d| d.unhoused as f32)),
            Series::new("waiting to move in", Color32::from_gray(160), pane::col(&days, |d| d.waiting_immigrants as f32)),
        ];
        pane::line_chart(ui, &labels, &series, 160.0, &|x| format!("{x:.0}"));
        pane::legend(ui, &series.iter().map(|s| (s.name.as_str(), s.color)).collect::<Vec<_>>());
    }
    heading(ui, "Who lives here");
    row(ui, "people", format!("{}   {} waiting to move in   {} unhoused   {} unemployed", w.citizens.len(), e.waiting_immigrants, e.unhoused, e.unemployed));
    class_row(ui, "by income class", e.class_counts);
    class_row(ui, "homes by tier", e.tier_counts);
    pane::row_with(ui, "rent per home per day", |ui| {
        for (i, t) in Tier::ALL.iter().enumerate() {
            ui.label(egui::RichText::new(format!("{} {:.0}", t.label(), e.avg_rent[i])).color(tier_color(*t)));
        }
    });
    heading(ui, "Pushed out");
    row(ui, "since the start", warn_if(e.displaced_total > 0, format!("{} displaced   {} left town   {} priced out", e.displaced_total, e.emigrated_total, e.priced_out_total)));
    // [review] Why they left, biggest reason first.
    if e.emigrated_total > 0 {
        let mut why: Vec<(u32, &str)> = nd_sim::economy::LeaveReason::ALL.iter().map(|r| (e.emigrated_by[r.index()], r.key())).filter(|(n, _)| *n > 0).collect();
        why.sort_by(|a, b| b.0.cmp(&a.0));
        if !why.is_empty() {
            row(ui, "left over", why.iter().map(|(n, k)| format!("{k} {n}")).collect::<Vec<_>>().join("   "));
            hint(ui, "People who left town, by reason (since this was counted).");
        }
    }
}

/// People, Income and inequality: the classes and the Gini index over the
/// range, then how income is spread now.
pub fn inequality_page(ui: &mut egui::Ui, p: &mut UiCtx) {
    let span = pane::data_span(ui.ctx());
    let w = &p.sim.world;
    let e = &w.economy;
    let mut incomes: Vec<f32> = w.citizens.values().map(|c| c.income).collect();
    incomes.sort_by(f32::total_cmp);
    let n = incomes.len();
    let total: f32 = incomes.iter().sum();
    pane::cards(ui, |ui| {
        pane::kpi(ui, "Gini index", egui::RichText::new(format!("{:.2}", e.gini)).color(if e.gini > 0.4 { WARN } else { GOLD }));
        if n > 0 {
            let top: f32 = incomes[n - (n / 10).max(1)..].iter().sum::<f32>() / total.max(1.0);
            pane::kpi(ui, "median income/day", egui::RichText::new(format!("{:.0}", incomes[n / 2])));
            pane::kpi(ui, "mean income/day", egui::RichText::new(format!("{:.0}", total / n as f32)));
            pane::kpi(ui, "earned by the top 10%", egui::RichText::new(format!("{:.0}%", top * 100.0)));
        }
        for (i, t) in Tier::ALL.iter().enumerate() {
            pane::kpi(ui, &format!("{} earners", t.label()), egui::RichText::new(format!("{}", e.class_counts[i])).color(tier_color(*t)));
        }
    });
    let days = pane::days(&e.history, if span == pane::Span::Hours { pane::Span::Week } else { span });
    let labels = pane::day_labels(&days);
    heading(ui, "By income class");
    let series: Vec<Series> = Tier::ALL.iter().enumerate().map(|(i, t)| Series::new(format!("{} earners", t.label()), tier_color(*t), pane::col(&days, |d| d.classes[i] as f32))).collect();
    pane::stacked_bars(ui, &labels, &series, 130.0, false, None, &|x| format!("{x:.0}"));
    pane::legend(ui, &series.iter().map(|s| (s.name.as_str(), s.color)).collect::<Vec<_>>());
    heading(ui, "Gini index over time");
    pane::line_chart(ui, &labels, &[Series::new("Gini", GOLD, pane::col(&days, |d| d.gini))], 100.0, &|x| format!("{x:.2}"));
    hint(ui, "0 is everyone earning the same; 1 is one person earning everything. Above 0.4 reads as unequal.");
    heading(ui, "How income is spread now");
    inequality(ui, w);
}

/// People, Favourites.
pub fn favourites_page(ui: &mut egui::Ui, p: &mut UiCtx) {
    let n = p.tools.favorites.iter().filter(|id| p.sim.world.citizens.contains_key(**id)).count();
    pane::cards(ui, |ui| {
        pane::kpi(ui, "starred", egui::RichText::new(format!("{n}")).color(GOLD));
        pane::kpi(ui, "gift from the treasury", egui::RichText::new(format!("${:.0}", nd_sim::GIFT_AMOUNT)));
    });
    heading(ui, "Your people");
    favourites(ui, p);
}

/// Three figures coloured by class, with their shares.
fn class_row(ui: &mut egui::Ui, label: &str, counts: [u32; 3]) {
    let total = counts.iter().sum::<u32>().max(1) as f32;
    pane::row_with(ui, label, |ui| {
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
        // The Lorenz chart beside it is LORENZ_SIZE wide; reserving less made
        // the window grow a little every frame until it filled the screen.
        let width = (ui.available_width() - LORENZ_SIZE - ui.spacing().item_spacing.x).clamp(220.0, 760.0);
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
    let plot = egui::Rect::from_min_max(rect.min + egui::vec2(36.0, 8.0), rect.max - egui::vec2(6.0, 19.0));
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

    let small = crate::ui::theme::small();
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
const LORENZ_SIZE: f32 = 160.0;

fn lorenz(ui: &mut egui::Ui, sorted: &[f32], total: f32) {
    let (resp, painter) = ui.allocate_painter(egui::vec2(LORENZ_SIZE, LORENZ_SIZE), egui::Sense::hover());
    let rect = resp.rect;
    let plot = egui::Rect::from_min_max(rect.min + egui::vec2(6.0, 6.0), rect.max - egui::vec2(6.0, 34.0));
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
    let small = crate::ui::theme::small();
    let grey = egui::Color32::from_gray(170);
    painter.text(egui::pos2(plot.center().x, plot.bottom() + 2.0), egui::Align2::CENTER_TOP, "people, poorest first", small.clone(), grey);
    painter.text(egui::pos2(plot.center().x, plot.bottom() + 17.0), egui::Align2::CENTER_TOP, "↑ share of income", small, grey);
    resp.on_hover_text("Lorenz curve: the further it sags below the diagonal, the more unequal the city. Gini is twice the area between them.");
}

/// Starred people, with a button to select and one to fly the camera to
/// them. Anyone who left town drops off the list.
fn favourites(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    p.tools.favorites.retain(|id| w.citizens.contains_key(*id));
    if p.tools.favorites.is_empty() {
        hint(ui, "Star someone in their person pane (Inspect tool, click a citizen) to keep them here.");
        return;
    }
    let mut pick: Option<CitizenId> = None;
    let mut jump: Option<CitizenId> = None;
    let mut drop: Option<CitizenId> = None;
    let mut follow: Option<CitizenId> = None;
    let mut gift: Option<CitizenId> = None;
    let can_gift = w.economy.treasury >= nd_sim::GIFT_AMOUNT;
    for &id in &p.tools.favorites {
        let Some(c) = w.citizens.get(id) else { continue };
        ui.horizontal(|ui| {
            let sel = *p.selection == Selection::Citizen(id);
            // [camera] Favourites wear gold, here as in the city.
            if ui.selectable_label(sel, egui::RichText::new(&c.name).color(super::GOLD)).clicked() {
                pick = Some(id);
            }
            hint(ui, c.activity.label());
            if ui.small_button("go").on_hover_text("Select them and fly the camera there smoothly").clicked() {
                pick = Some(id);
                jump = Some(id);
            }
            let following = p.follow.is(Selection::Citizen(id));
            if ui.add(egui::Button::new(if following { "stop" } else { "follow" }).small().selected(following)).on_hover_text(if following { "Stop following" } else { "Follow them with the camera" }).clicked() {
                follow = Some(id);
            }
            if ui.add_enabled(can_gift, egui::Button::new("$").small()).on_hover_text(format!("Gift ${:.0} from the treasury to their household", nd_sim::GIFT_AMOUNT)).clicked() {
                gift = Some(id);
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
    if let Some(id) = jump {
        super::follow::zoom_to(p.sim, Selection::Citizen(id), p.follow, p.fly, p.mode);
    }
    if let Some(id) = follow {
        if p.follow.is(Selection::Citizen(id)) {
            p.follow.stop();
        } else {
            super::follow::start_follow(p.sim, Selection::Citizen(id), p.follow, p.fly, None);
        }
    }
    if let Some(id) = gift {
        let name = p.sim.world.citizens.get(id).map(|c| c.name.clone()).unwrap_or_default();
        p.flags.message = match p.sim.world.gift(id, nd_sim::GIFT_AMOUNT) {
            Ok(g) => format!("Gave ${:.0} to {name}'s household. Treasury {:.0}.", g.amount, g.treasury),
            Err(e) => format!("No gift: {e}."),
        };
    }
}
