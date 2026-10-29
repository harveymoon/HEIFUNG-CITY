//! [r4-timetable] A line's day: the timeline chart (riders boarding each
//! hour today against yesterday, riders waiting and on board through the
//! day, over the timetable's parts shaded behind) and the timetable editor
//! under it (presets, the night service, each part's headway or buses, the
//! wait riders had in each part, what a day of it runs and costs). The
//! part edges drag on the chart, a quarter hour at a time.
//!
//! The figures are the sim's own (`nd_sim::timetable::Timeline`), so the
//! remote API's `timetable` reads the same.

use bevy_egui::egui::{self, Color32, Stroke};
use nd_sim::timetable::{hm, Kind, Part, Preset, Timeline, Timetable, SNAP};

use crate::ui::pane::{self, Band, Series};
use crate::ui::theme::{AMBER, TEAL};
use crate::ui::{GOLD, IVORY, WARN};

/// What the editor asks for.
pub enum Edit {
    /// A timetable of the line's own, or `None`: back to the standard.
    Set(Option<Timetable>),
    Preset(Preset),
}

/// A part's colour on the chart and in the editor.
pub fn part_color(p: Part) -> Color32 {
    match p {
        Part::MorningRush => AMBER,
        Part::Day => TEAL,
        Part::EveningRush => Color32::from_rgb(250, 140, 80),
        Part::Evening => Color32::from_rgb(190, 130, 230),
        Part::Night => Color32::from_rgb(90, 120, 240),
    }
}

/// A part's value, short, for the bands: "10'", "3 b".
fn short(kind: Kind, v: u32) -> String {
    if kind.per_bus() {
        format!("{v} bus{}", if v == 1 { "" } else { "es" })
    } else {
        format!("{v}'")
    }
}

/// The stretches of the day a part runs, in words: "06:00–09:00".
fn part_hours(t: &Timetable, p: Part) -> String {
    let spans: Vec<String> = t.segments().into_iter().filter(|s| s.2 == Some(p)).map(|(a, b, _)| format!("{}–{}", hm(a), hm(b))).collect();
    if spans.is_empty() {
        "—".into()
    } else {
        spans.join(", ")
    }
}

fn slot_values(tl: &[nd_sim::timetable::Slot], f: impl Fn(&nd_sim::timetable::Slot) -> f32) -> Vec<f32> {
    (0..24).map(|h| tl.get(h).map_or(0.0, &f)).collect()
}

/// The chart of a line's day, with the part edges as handles to drag when
/// `edit` is set. Returns the timetable with an edge moved, while one is
/// being dragged.
pub fn chart(ui: &mut egui::Ui, id: egui::Id, kind: Kind, t: &Timetable, tl: &Timeline, now: u32, edit: bool) -> Option<Timetable> {
    let bands: Vec<Band> = t.segments().into_iter().filter_map(|(a, b, p)| p.map(|p| Band { from: a, to: b, color: part_color(p), label: short(kind, t.value(p)) })).collect();
    let bars = [
        Series::new("boarded today", GOLD, slot_values(&tl.today, |s| s.boarded as f32)),
        Series::new("boarded yesterday", Color32::from_gray(150), slot_values(&tl.yesterday, |s| s.boarded as f32)),
    ];
    let lines = [
        Series::new("waiting (average)", WARN, slot_values(&tl.today, |s| s.mean_waiting())),
        Series::new("on board (average)", Color32::from_rgb(120, 230, 150), slot_values(&tl.today, |s| s.mean_riding())),
    ];
    let plot = pane::day_chart(ui, &bands, &bars, &lines, Some(now), 150.0, &|x| format!("{x:.0}"));
    pane::legend(ui, &[("boarded today", GOLD), ("yesterday", Color32::from_gray(150)), ("waiting", WARN), ("on board", Color32::from_rgb(120, 230, 150))]);
    if !edit {
        return None;
    }
    // The edges: service start, the rushes' ends, service end.
    let x_of = |m: u32| plot.left() + plot.width() * m as f32 / 1440.0;
    let edges = [t.start, t.am.0, t.am.1, t.pm.0, t.pm.1, t.end];
    let names = ["first run", "morning rush begins", "morning rush ends", "evening rush begins", "evening rush ends", "last run"];
    let mut out = None;
    for (k, m) in edges.iter().copied().enumerate() {
        let x = x_of(m);
        let r = egui::Rect::from_center_size(egui::pos2(x, plot.top() - 7.0), egui::vec2(9.0, 14.0));
        let resp = ui.interact(r, id.with(("edge", k)), egui::Sense::drag());
        let hot = resp.hovered() || resp.dragged();
        let col = if hot { GOLD } else { IVORY.gamma_multiply(0.75) };
        ui.painter().line_segment([egui::pos2(x, plot.top() - 14.0), egui::pos2(x, plot.bottom())], Stroke::new(if hot { 2.0 } else { 1.0 }, col.gamma_multiply(0.6)));
        ui.painter().rect_filled(r.shrink(1.0), 2.0, col);
        if hot {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        let resp = resp.on_hover_text(format!("{} {} — drag to move it (a quarter hour at a time)", names[k], hm(m)));
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                let raw = ((p.x - plot.left()) / plot.width() * 1440.0).clamp(0.0, 1440.0);
                let m = ((raw / SNAP as f32).round() as u32 * SNAP).min(1440);
                let mut n = *t;
                match k {
                    0 => n.start = m,
                    1 => n.am.0 = m,
                    2 => n.am.1 = m,
                    3 => n.pm.0 = m,
                    4 => n.pm.1 = m,
                    _ => n.end = m,
                }
                let n = n.clamped(kind);
                if n != *t {
                    out = Some(n);
                }
            }
        }
    }
    out
}

/// The timetable editor: presets, the night, and each part's dial with
/// the riders and waits it had. `custom`: the line has its own timetable.
pub fn editor(ui: &mut egui::Ui, id: egui::Id, kind: Kind, t: &Timetable, custom: bool, tl: &Timeline) -> Option<Edit> {
    let mut out = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(format!("{} · {}", t.hours_text(), if custom { t.preset(kind).map_or("own timetable", |p| p.label()) } else { "standard (one frequency all day)" })).small().color(IVORY));
    });
    ui.horizontal_wrapped(|ui| {
        for p in Preset::ALL {
            let on = custom && t.preset(kind) == Some(p);
            if ui.selectable_label(on, egui::RichText::new(p.label()).small()).on_hover_text(p.blurb()).clicked() {
                out = Some(Edit::Preset(p));
            }
        }
        if ui.selectable_label(!custom, egui::RichText::new("Standard").small()).on_hover_text("As every line ran before timetables: the standard hours, one frequency all day").clicked() && custom {
            out = Some(Edit::Set(None));
        }
    });
    // The night.
    let mode = if !t.night { 0 } else if t.value(Part::Night) == t.value(Part::Day) { 2 } else { 1 };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("night").small().weak());
        for (k, (label, tip)) in [("off", "Nothing runs outside the hours"), ("sparse", "A thinner service through the night"), ("24 hours", "The day's frequency round the clock")].into_iter().enumerate() {
            if ui.selectable_label(mode == k, egui::RichText::new(label).small()).on_hover_text(tip).clicked() && mode != k {
                let mut n = *t;
                n.night = k > 0;
                let day = t.value(Part::Day);
                n.values[Part::Night.index()] = match k {
                    2 => day,
                    1 => Preset::NightOwl.timetable(kind, day).value(Part::Night),
                    _ => n.values[Part::Night.index()],
                };
                out = Some(Edit::Set(Some(n.clamped(kind))));
            }
        }
    });
    // Each part: when, how often, who rode and how long they waited.
    let today = tl.by_part(t, false);
    let yday = tl.by_part(t, true);
    let (lo, hi) = kind.limits();
    egui::Grid::new(id.with("parts")).num_columns(5).spacing(egui::vec2(8.0, 2.0)).striped(true).show(ui, |ui| {
        for h in ["part", "hours", if kind.per_bus() { "buses" } else { "headway" }, "boarded", "avg wait"] {
            ui.label(egui::RichText::new(h).small().weak());
        }
        ui.end_row();
        for p in Part::ALL {
            let runs = p != Part::Night || t.night;
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(dot, 2.0, part_color(p).gamma_multiply(if runs { 1.0 } else { 0.3 }));
                ui.label(egui::RichText::new(p.label()).small().color(if runs { IVORY } else { Color32::from_gray(120) }));
            });
            ui.label(egui::RichText::new(part_hours(t, p)).small());
            ui.horizontal(|ui| {
                let v = t.value(p);
                if ui.add_enabled(runs, egui::Button::new(egui::RichText::new("−").small()).small()).on_hover_text(if kind.per_bus() { "A bus fewer in this part" } else { "Less often" }).clicked() {
                    let mut n = *t;
                    n.values[p.index()] = if kind.per_bus() { v.saturating_sub(1).max(lo) } else { (v + kind.step()).min(hi) };
                    out = Some(Edit::Set(Some(n)));
                }
                ui.label(egui::RichText::new(kind.value_text(v)).small().color(if runs { GOLD } else { Color32::from_gray(120) }));
                if ui.add_enabled(runs, egui::Button::new(egui::RichText::new("+").small()).small()).on_hover_text(if kind.per_bus() { "A bus more in this part" } else { "More often" }).clicked() {
                    let mut n = *t;
                    n.values[p.index()] = if kind.per_bus() { (v + 1).min(hi) } else { v.saturating_sub(kind.step()).max(lo) };
                    out = Some(Edit::Set(Some(n)));
                }
            });
            let (b, wt) = today[p.index()];
            let (by, wy) = yday[p.index()];
            ui.label(egui::RichText::new(format!("{b} ({by})")).small()).on_hover_text("Today, and yesterday in brackets");
            let w = |x: Option<f32>| x.map_or("—".to_string(), |m| format!("{m:.0} min"));
            ui.label(egui::RichText::new(format!("{} ({})", w(wt), w(wy))).small()).on_hover_text("From reaching the stop to boarding: today, and yesterday in brackets");
            ui.end_row();
        }
    });
    // What a day of it runs and costs.
    let work = t.daily_work(kind);
    let standard = Timetable::flat(kind, t.value(Part::Day)).daily_work(kind);
    let (b, w, missed, _) = tl.totals(true);
    let text = match kind {
        Kind::Bus => format!("{:.0} bus-hours a day (standard {:.0}): about {:.0} a day in running costs", work / 60.0, standard / 60.0, work / (nd_sim::transit::SERVICE_END - nd_sim::transit::SERVICE_START) as f32 * nd_sim::transit::BUS_UPKEEP_PER_DAY),
        Kind::Metro => format!("{:.0} trains a day each way (standard {:.0}): about {:.0} a day in runs", work, standard, 2.0 * work * nd_sim::subway::RUN_COST),
        Kind::Rail => format!("{:.0} trains a day (standard {:.0}); each beyond the standard costs {:.0}", work, Timetable::flat(kind, kind.standard()).daily_work(kind), nd_sim::world::timetables::RAIL_RUN_COST),
        Kind::Ferry => format!("{:.0} sailings a day (standard {:.0}); each beyond the standard costs {:.0}", work, Timetable::flat(kind, kind.standard()).daily_work(kind).ceil(), nd_sim::world::timetables::FERRY_RUN_COST),
    };
    ui.label(egui::RichText::new(text).small().weak());
    ui.label(egui::RichText::new(format!("yesterday: {b} boarded, average wait {}, {missed} left waiting after the last run", w.map_or("—".to_string(), |m| format!("{m:.0} min")))).small().weak());
    out
}

/// Both, for a line: the chart and the editor (`edit` off: the chart and
/// the parts' figures only).
pub fn show(ui: &mut egui::Ui, id: egui::Id, kind: Kind, t: &Timetable, custom: bool, tl: &Timeline, now: u32, edit: bool) -> Option<Edit> {
    let dragged = chart(ui, id, kind, t, tl, now, edit).map(|n| Edit::Set(Some(n)));
    let edited = if edit { editor(ui, id, kind, t, custom, tl) } else { None };
    dragged.or(edited)
}
