//! [concerns] City Data's People, Complaints and concerns page: city
//! happiness over time as a line chart (mean mood, the unhappy and happy
//! shares, mood by class in the tier colours) and the concerns people
//! raise most this week, ranked, each with a trend against last week, a
//! bar, where it comes from and the people's own words. Drawn with the
//! painter like the income histogram; reads `World::concerns` only.

use bevy::prelude::*;
use bevy_egui::egui;
use nd_sim::concerns::{Concern, MoodSample, Ranked, HAPPY, UNHAPPY, WEEK};
use nd_sim::{CitizenId, Tier, World};

use super::people::tier_color;
use super::pane::heading;
use super::{hint, UiCtx, GOLD, IVORY, WARN};
use crate::tools::Selection;

const CALM: egui::Color32 = egui::Color32::from_rgb(120, 210, 140);
const GRID: egui::Color32 = egui::Color32::from_gray(80);
const PANEL: egui::Color32 = super::theme::PLOT_BG;

/// How far back the chart looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Range {
    Hours,
    #[default]
    Week,
    Month,
    All,
}

impl Range {
    const ALL: [Range; 4] = [Range::Hours, Range::Week, Range::Month, Range::All];

    fn label(self) -> &'static str {
        match self {
            Range::Hours => "48 h",
            Range::Week => "7 days",
            Range::Month => "30 days",
            Range::All => "all",
        }
    }

    fn days(self) -> Option<u64> {
        match self {
            Range::Hours => None,
            Range::Week => Some(7),
            Range::Month => Some(30),
            Range::All => Some(u64::MAX),
        }
    }
}

fn mem_id(what: &str) -> egui::Id {
    egui::Id::new(("people_concerns", what))
}

fn mood_color(m: f32) -> egui::Color32 {
    if m < UNHAPPY {
        WARN
    } else if m > HAPPY {
        CALM
    } else {
        GOLD
    }
}

pub fn tab(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let now = w.mood_now();
    if now.people > 0 {
        let top = w.concerns.ranked(w.clock.day()).into_iter().next();
        super::pane::cards(ui, |ui| {
            super::pane::kpi(ui, "mean mood", egui::RichText::new(format!("{:.0}%", now.mean * 100.0)).color(mood_color(now.mean)));
            super::pane::kpi(ui, "unhappy", egui::RichText::new(format!("{:.0}%", now.unhappy * 100.0)).color(if now.unhappy > 0.2 { WARN } else { IVORY }));
            super::pane::kpi(ui, "happy", egui::RichText::new(format!("{:.0}%", now.happy * 100.0)).color(CALM));
            if let Some(r) = top {
                super::pane::kpi(ui, "top concern this week", egui::RichText::new(format!("{} ({})", r.concern.label(), r.this_week)).color(GOLD));
            }
        });
    }
    heading(ui, "Happiness");
    if now.people == 0 {
        hint(ui, "Nobody lives here yet.");
        return;
    }
    if let Some(prev) = w.concerns.mood_days.back() {
        let d = (now.mean - prev.mean) * 100.0;
        if d.abs() >= 0.5 {
            ui.label(egui::RichText::new(format!("mean mood {}{:.0} pts since midnight", if d > 0.0 { "+" } else { "-" }, d.abs())).color(if d > 0.0 { CALM } else { WARN }));
        }
    }
    let ctx = ui.ctx().clone();
    let mut range: Range = ctx.data(|d| d.get_temp(mem_id("range"))).unwrap_or_default();
    ui.horizontal(|ui| {
        for r in Range::ALL {
            ui.selectable_value(&mut range, r, r.label());
        }
    });
    ctx.data_mut(|d| d.insert_temp(mem_id("range"), range));
    let samples: Vec<MoodSample> = match range.days() {
        None => w.concerns.mood_hours.iter().copied().chain(std::iter::once(now)).collect(),
        Some(days) => {
            let from = w.clock.day().saturating_sub(days);
            w.concerns.mood_days.iter().filter(|s| s.day() >= from).copied().chain(std::iter::once(now)).collect()
        }
    };
    chart(ui, &samples, range);
    legend(ui);

    heading(ui, "Top concerns this week");
    concerns(ui, p);
}

fn legend(ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        // A swatch drawn, not a glyph: the UI font has no box-drawing characters.
        let item = |ui: &mut egui::Ui, c: egui::Color32, t: &str| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 10.0), egui::Sense::hover());
            ui.painter().line_segment([r.left_center(), r.right_center()], egui::Stroke::new(2.5, c));
            ui.label(egui::RichText::new(t).small().color(c));
        };
        item(ui, IVORY, "mean mood");
        item(ui, WARN, "share unhappy");
        item(ui, CALM, "share happy");
        for t in Tier::ALL {
            item(ui, tier_color(t), &format!("{} class", t.label()));
        }
    });
}

/// Mood and shares on one 0..1 axis, oldest left; the dashed lines are
/// the unhappy and happy thresholds.
fn chart(ui: &mut egui::Ui, samples: &[MoodSample], range: Range) {
    let width = ui.available_width().max(260.0);
    let (resp, painter) = ui.allocate_painter(egui::vec2(width, 160.0), egui::Sense::hover());
    let rect = resp.rect;
    let plot = egui::Rect::from_min_max(rect.min + egui::vec2(38.0, 8.0), rect.max - egui::vec2(8.0, 19.0));
    painter.rect_filled(plot, 2.0, PANEL);
    let small = crate::ui::theme::small();
    let grey = egui::Color32::from_gray(170);
    let y_of = |v: f32| plot.bottom() - plot.height() * v.clamp(0.0, 1.0);
    for (v, label) in [(0.0, "0"), (0.5, "50%"), (1.0, "100%")] {
        painter.line_segment([egui::pos2(plot.left(), y_of(v)), egui::pos2(plot.right(), y_of(v))], egui::Stroke::new(1.0, GRID));
        painter.text(egui::pos2(plot.left() - 3.0, y_of(v)), egui::Align2::RIGHT_CENTER, label, small.clone(), grey);
    }
    for (v, c) in [(UNHAPPY, WARN), (HAPPY, CALM)] {
        dashed_hline(&painter, y_of(v), plot.x_range(), c.gamma_multiply(0.45));
    }
    if samples.len() < 2 {
        painter.text(plot.center(), egui::Align2::CENTER_CENTER, "Happiness is recorded every hour and at midnight.\nCome back in a little while.", small, grey);
        return;
    }
    let (t0, t1) = (samples[0].tick as f32, samples[samples.len() - 1].tick as f32);
    let span = (t1 - t0).max(1.0);
    let x_of = |t: u64| plot.left() + plot.width() * (t as f32 - t0) / span;
    let line = |f: &dyn Fn(&MoodSample) -> Option<f32>, color: egui::Color32, width: f32| {
        let pts: Vec<egui::Pos2> = samples.iter().filter_map(|s| f(s).map(|v| egui::pos2(x_of(s.tick), y_of(v)))).collect();
        if pts.len() >= 2 {
            painter.add(egui::Shape::line(pts, egui::Stroke::new(width, color)));
        }
    };
    for t in Tier::ALL {
        line(&|s: &MoodSample| s.class(t), tier_color(t).gamma_multiply(0.8), 1.0);
    }
    line(&|s: &MoodSample| Some(s.unhappy), WARN, 1.5);
    line(&|s: &MoodSample| Some(s.happy), CALM, 1.5);
    line(&|s: &MoodSample| Some(s.mean), IVORY, 2.5);
    let first = samples[0];
    let start = match range {
        Range::Hours => format!("day {} {:02}:00", first.day() + 1, (first.tick % 1440) / 60),
        _ => format!("day {}", first.day() + 1),
    };
    painter.text(egui::pos2(plot.left(), plot.bottom() + 2.0), egui::Align2::LEFT_TOP, start, small.clone(), grey);
    painter.text(egui::pos2(plot.right(), plot.bottom() + 2.0), egui::Align2::RIGHT_TOP, "now", small, grey);
    // Hover: the nearest sample, with a cursor line.
    if let Some(pos) = resp.hover_pos().filter(|p| plot.contains(*p)) {
        let s = samples.iter().min_by(|a, b| (x_of(a.tick) - pos.x).abs().total_cmp(&(x_of(b.tick) - pos.x).abs())).copied().unwrap();
        let x = x_of(s.tick);
        painter.line_segment([egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())], egui::Stroke::new(1.0, egui::Color32::from_gray(150)));
        painter.circle_filled(egui::pos2(x, y_of(s.mean)), 3.0, IVORY);
        let class = Tier::ALL.iter().filter_map(|t| s.class(*t).map(|m| format!("{} {:.0}%", t.label(), m * 100.0))).collect::<Vec<_>>().join(" · ");
        resp.on_hover_text(format!(
            "day {} {:02}:{:02} · {} people\nmean mood {:.0}%\nunhappy {:.0}% · happy {:.0}%\n{class}",
            s.day() + 1,
            (s.tick % 1440) / 60,
            s.tick % 60,
            s.people,
            s.mean * 100.0,
            s.unhappy * 100.0,
            s.happy * 100.0
        ));
    }
}

fn dashed_hline(painter: &egui::Painter, y: f32, x: egui::Rangef, color: egui::Color32) {
    let mut x0 = x.min;
    while x0 < x.max {
        let x1 = (x0 + 5.0).min(x.max);
        painter.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], egui::Stroke::new(1.0, color));
        x0 += 9.0;
    }
}

/// "+40%" in red, "-20%" in green, "steady" grey (ASCII: the UI font has no arrows).
fn trend_text(r: &Ranked) -> egui::RichText {
    match r.direction() {
        1 if r.last_week == 0 => egui::RichText::new("new").color(WARN),
        1 => egui::RichText::new(format!("+{:.0}%", r.trend() * 100.0)).color(WARN),
        -1 => egui::RichText::new(format!("-{:.0}%", -r.trend() * 100.0)).color(CALM),
        _ => egui::RichText::new("steady").color(egui::Color32::from_gray(150)),
    }
    .small()
}

/// The ranked concerns. A row opens to show where it comes from and what
/// people said; a quote selects its speaker and flies to where it was said.
fn concerns(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w: &World = &p.sim.world;
    let today = w.clock.day();
    let ranked = w.concerns.ranked(today);
    if ranked.is_empty() {
        hint(ui, "No complaints in the last two weeks. Enjoy it while it lasts.");
        return;
    }
    let ctx = ui.ctx().clone();
    // The top concern starts open, until the player opens or closes a row.
    let mut open: Option<Concern> = ctx.data(|d| d.get_temp::<Option<Concern>>(mem_id("open"))).unwrap_or(Some(ranked[0].concern));
    let peak = ranked.iter().map(|r| r.this_week.max(r.last_week)).max().unwrap_or(1).max(1);
    let mut pick: Option<CitizenId> = None;
    let mut jump: Option<nd_sim::Vec2> = None;
    for r in &ranked {
        let is_open = open == Some(r.concern);
        ui.horizontal(|ui| {
            let label = egui::RichText::new(format!("{} {}", if is_open { "-" } else { "+" }, r.concern.label()));
            let resp = ui.add_sized([150.0, 18.0], egui::Button::selectable(is_open, label)).on_hover_text(format!(
                "{}\n{} people this week, {} last week, {} today, {} all time\nmean severity {:.0}%",
                r.concern.about(),
                r.this_week,
                r.last_week,
                r.today,
                r.total,
                r.severity * 100.0
            ));
            if resp.clicked() {
                open = if is_open { None } else { Some(r.concern) };
            }
            bar(ui, r, peak);
            ui.label(egui::RichText::new(format!("{}", r.this_week)).strong());
            ui.label(trend_text(r));
        });
        if !is_open {
            continue;
        }
        ui.indent(r.concern.key(), |ui| {
            if let Some((at, n)) = w.concerns.hotspot(r.concern, today, WEEK) {
                let near = w.building_nearest(at).and_then(|b| w.buildings.get(b)).map(|b| b.name.clone()).unwrap_or_else(|| format!("[{:.0}, {:.0}]", at.x, at.y));
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("where").weak().small());
                    if ui.link(egui::RichText::new(format!("around {near}")).small()).on_hover_text(format!("{n} of this week's complaints came from within {:.0} m of here. Click to fly there.", nd_sim::concerns::SPOT_CELL)).clicked() {
                        jump = Some(at);
                    }
                    hint(ui, &format!("({n} this week)"));
                });
            }
            let mut any = false;
            for q in w.concerns.quotes(r.concern) {
                any = true;
                let here = w.citizens.contains_key(q.citizen);
                let who = if here { q.name.clone() } else { format!("{} (left town)", q.name) };
                let text = egui::RichText::new(format!("\"{}\"", q.text)).italics().small();
                ui.horizontal_wrapped(|ui| {
                    let sel = here && *p.selection == Selection::Citizen(q.citizen);
                    let resp = ui.selectable_label(sel, text).on_hover_text(if here { "Select them and fly to where they said it" } else { "They have left town: fly to where they said it" });
                    if resp.clicked() {
                        if here {
                            pick = Some(q.citizen);
                        }
                        jump = Some(q.pos);
                    }
                    hint(ui, &format!("{who}, {}", super::fmt::day_time(q.tick)));
                });
            }
            if !any {
                hint(ui, "No recent quotes.");
            }
        });
    }
    ctx.data_mut(|d| d.insert_temp(mem_id("open"), open));
    hint(ui, "People complaining per concern over the last 7 days (each counts once a day); the arrow compares with the 7 days before. Open a row for where and what they said.");
    if let Some(id) = pick {
        *p.selection = Selection::Citizen(id);
    }
    if let Some(pos) = jump {
        p.fly_to = Some(Vec2::new(pos.x, pos.y));
    }
}

/// This week's bar, last week's as a tick, coloured by severity.
fn bar(ui: &mut egui::Ui, r: &Ranked, peak: u32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(110.0, 12.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, PANEL);
    let wv = rect.width() * r.this_week as f32 / peak as f32;
    let color = super::theme::lerp_floor(GOLD, WARN, r.severity.clamp(0.0, 1.0));
    painter.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(wv.max(1.0), rect.height())), 2.0, color);
    let lx = rect.left() + rect.width() * r.last_week as f32 / peak as f32;
    painter.line_segment([egui::pos2(lx, rect.top() - 1.0), egui::pos2(lx, rect.bottom() + 1.0)], egui::Stroke::new(1.5, IVORY));
}
