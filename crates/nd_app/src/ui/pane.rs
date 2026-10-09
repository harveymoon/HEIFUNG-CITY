//! What the data pages and the tool windows share: the tool windows' header
//! (title and close), the City Data view's page furniture (page title,
//! headline cards, section headings, figure rows, tables), the time-range
//! picker, and the charts.
//!
//! Every City Data page (`ui/city_data.rs`) is laid out the same way, top
//! to bottom: a page title with a line saying what the page is for, a row
//! of headline figures as cards (`kpi`), then sections under one kind of
//! heading (`heading`), each with full-width charts (`line_chart`,
//! `stacked_bars`, a `legend` under them), tables (`table`) and labelled
//! figures (`row`). The sizes come from `ui/theme.rs`.
//!
//! The charts are painted by hand (no plotting crate): a line chart and a
//! diverging stacked-bar chart, both with a hover readout of every series
//! at the day under the pointer. A few hundred shapes a frame, and only
//! while a page with charts is showing.

use bevy_egui::egui::{self, Color32, Pos2, Rect, Stroke};
use nd_sim::history::{DayRecord, HourRecord, History};

use super::theme::PLOT_BG;
use super::{Info, GOLD, IVORY};

// ── tool window header ─────────────────────────────────────────────────────

/// A tool window's header: its title and, at the right, close. Returns
/// whether the window should stay open.
pub fn header(ui: &mut egui::Ui, info: Info) -> bool {
    title_bar(ui, info.title(), None)
}

/// The same header for a window that is not an `Info` window (the Build
/// window, the title screen): its title, an optional weak note, and close.
/// Returns whether the window should stay open.
pub fn title_bar(ui: &mut egui::Ui, title: &str, note: Option<&str>) -> bool {
    let mut keep = true;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).color(GOLD).strong());
        if let Some(n) = note {
            ui.label(egui::RichText::new(n).small().weak());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if close_button(ui).on_hover_text("Close").clicked() {
                keep = false;
            }
        });
    });
    ui.separator();
    keep
}

/// A painted cross: the default font has no multiplication-x glyph.
pub fn close_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 3.0, ui.visuals().widgets.hovered.bg_fill);
    }
    let stroke = Stroke::new(1.5, if resp.hovered() { GOLD } else { Color32::from_gray(190) });
    let r = rect.shrink(5.0);
    p.line_segment([r.left_top(), r.right_bottom()], stroke);
    p.line_segment([r.right_top(), r.left_bottom()], stroke);
    resp
}

// ── page furniture ─────────────────────────────────────────────────────────

/// A card's fill and edge: a shade lighter than the page.
const CARD_BG: Color32 = Color32::from_rgb(18, 21, 33);
const CARD_EDGE: Color32 = Color32::from_rgb(52, 46, 38);
/// Table and figure captions: quieter than the figures.
pub const CAPTION: Color32 = Color32::from_gray(160);
/// Width of a figure row's label column, so the values line up.
const ROW_LABEL: f32 = 190.0;

/// The page's title and one line on what it is for.
pub fn page_head(ui: &mut egui::Ui, title: &str, blurb: &str) {
    ui.label(egui::RichText::new(title).font(crate::ui::theme::title()).color(GOLD));
    ui.label(egui::RichText::new(blurb).color(CAPTION));
    ui.add_space(6.0);
}

/// A section heading: the display face over a hairline rule, with air above.
pub fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(14.0);
    ui.label(egui::RichText::new(text).font(crate::ui::theme::lead()).color(GOLD));
    let y = ui.cursor().top() + 1.0;
    let x = ui.max_rect().x_range();
    ui.painter().hline(x, y, Stroke::new(1.0, CARD_EDGE));
    ui.add_space(6.0);
}

/// A headline figure as a card: the figure big, a caption under it. Lay
/// them out in a `ui.horizontal_wrapped` row at the top of a page.
pub fn kpi(ui: &mut egui::Ui, caption: &str, value: egui::RichText) {
    egui::Frame::new().fill(CARD_BG).stroke(Stroke::new(1.0, CARD_EDGE)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(12, 7)).show(ui, |ui| {
        ui.set_min_width(118.0);
        ui.vertical(|ui| {
            ui.label(value.font(egui::FontId::proportional(crate::ui::theme::HEADING)).strong());
            ui.label(egui::RichText::new(caption).small().color(CAPTION));
        });
    });
}

/// The row of headline cards.
pub fn cards(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        add(ui);
    });
}

/// A labelled figure on one line: the label in a fixed column, quiet; the
/// value plain.
pub fn row(ui: &mut egui::Ui, label: &str, value: impl Into<egui::RichText>) {
    let value = value.into();
    row_with(ui, label, |ui| {
        ui.label(value);
    });
}

/// A labelled line whose right side `add` lays out (several coloured
/// figures, a button).
pub fn row_with(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        let h = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(egui::vec2(ROW_LABEL, h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(ROW_LABEL);
            ui.add(egui::Label::new(egui::RichText::new(label).color(CAPTION)).truncate());
        });
        add(ui);
    });
}

/// A column heading in a `table`.
pub fn th(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(egui::RichText::new(text).small().strong().color(CAPTION))
}

/// A table: striped rows, the same spacing everywhere, the headings first.
/// `body` adds the rows (cells, then `ui.end_row()`).
pub fn table<R>(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, heads: &[&str], body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Grid::new(id).num_columns(heads.len().max(1)).spacing(egui::vec2(16.0, 4.0)).striped(true).show(ui, |ui| {
        if !heads.is_empty() {
            for h in heads {
                th(ui, h);
            }
            ui.end_row();
        }
        body(ui)
    })
    .inner
}

// ── time range ─────────────────────────────────────────────────────────────

/// How far back the charts of an expanded window look. `Hours` uses the
/// hourly ring where a chart has hourly data; daily charts show the last
/// three days then.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Span {
    Hours,
    Week,
    #[default]
    Month,
    Quarter,
    All,
}

impl Span {
    pub const ALL: [Span; 5] = [Span::Hours, Span::Week, Span::Month, Span::Quarter, Span::All];

    pub fn label(self) -> &'static str {
        match self {
            Span::Hours => "72 h",
            Span::Week => "7 d",
            Span::Month => "30 d",
            Span::Quarter => "90 d",
            Span::All => "all",
        }
    }

    /// Days a daily chart covers.
    pub fn days(self) -> usize {
        match self {
            Span::Hours => 3,
            Span::Week => 7,
            Span::Month => 30,
            Span::Quarter => 90,
            Span::All => usize::MAX,
        }
    }
}

/// The range buttons. `key` names whose setting it is (the City Data view
/// keeps one for all its pages, in egui memory).
pub fn span_picker(ui: &mut egui::Ui, key: &str) -> Span {
    let id = egui::Id::new(("info_span", key));
    let mut span: Span = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("range").small().color(CAPTION));
        for s in Span::ALL {
            ui.selectable_value(&mut span, s, egui::RichText::new(s.label()).small());
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, span));
    span
}

/// The range the City Data view's charts show, as last picked.
pub fn data_span(ctx: &egui::Context) -> Span {
    ctx.data(|d| d.get_temp(egui::Id::new(("info_span", DATA_SPAN)))).unwrap_or_default()
}

/// The City Data view's one range setting.
pub const DATA_SPAN: &str = "city_data";

/// The days a daily chart shows, oldest first.
pub fn days(h: &History, span: Span) -> Vec<&DayRecord> {
    h.last_days(span.days()).collect()
}

pub fn hours(h: &History) -> Vec<&HourRecord> {
    h.last_hours(nd_sim::history::HOURS_KEPT).collect()
}

pub fn day_labels(days: &[&DayRecord]) -> Vec<String> {
    days.iter().map(|d| format!("day {}", d.day)).collect()
}

pub fn hour_labels(hours: &[&HourRecord]) -> Vec<String> {
    hours.iter().map(|h| format!("day {} {:02}:00", h.day, h.hour)).collect()
}

// ── charts ─────────────────────────────────────────────────────────────────

/// One named, coloured line or stack layer.
pub struct Series {
    pub name: String,
    pub color: Color32,
    pub values: Vec<f32>,
}

impl Series {
    pub fn new(name: impl Into<String>, color: Color32, values: Vec<f32>) -> Self {
        Self { name: name.into(), color, values }
    }
}

const AXIS: Color32 = Color32::from_gray(110);
const TICK: Color32 = Color32::from_gray(170);

/// A "nice" axis top above `x`: 1, 2 or 5 times a power of ten.
fn nice(x: f32) -> f32 {
    if x <= 0.0 {
        return 1.0;
    }
    let p = 10f32.powf(x.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if m * p >= x {
            return m * p;
        }
    }
    10.0 * p
}

/// The plot rect inside an allocated chart, leaving room for the y labels
/// on the left and the x labels below.
fn plot_rect(rect: Rect) -> Rect {
    Rect::from_min_max(rect.min + egui::vec2(46.0, 4.0), rect.max - egui::vec2(6.0, 18.0))
}

fn empty(ui: &mut egui::Ui, height: f32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height.min(40.0)), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, PLOT_BG);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, crate::ui::theme::small(), TICK);
}

/// Index of the x slot under the pointer, if it is over the plot.
fn hovered_slot(resp: &egui::Response, plot: Rect, n: usize) -> Option<usize> {
    let pos = resp.hover_pos().filter(|p| plot.expand2(egui::vec2(0.0, 4.0)).contains(*p))?;
    let t = ((pos.x - plot.left()) / plot.width()).clamp(0.0, 0.9999);
    Some(((t * n as f32) as usize).min(n - 1))
}

fn x_labels(painter: &egui::Painter, plot: Rect, labels: &[String]) {
    let small = crate::ui::theme::small();
    if let (Some(first), Some(last)) = (labels.first(), labels.last()) {
        painter.text(egui::pos2(plot.left(), plot.bottom() + 2.0), egui::Align2::LEFT_TOP, first, small.clone(), TICK);
        if labels.len() > 1 {
            painter.text(egui::pos2(plot.right(), plot.bottom() + 2.0), egui::Align2::RIGHT_TOP, last, small, TICK);
        }
    }
}

/// The legend: a swatch and a name per series, wrapping.
pub fn legend(ui: &mut egui::Ui, series: &[(&str, Color32)]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (name, col) in series {
            let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 1.0, *col);
            ui.label(egui::RichText::new(*name).small());
            ui.add_space(6.0);
        }
    });
}

/// The hover readout: the slot's label, then every series' value there.
fn readout(resp: egui::Response, label: &str, rows: Vec<(String, Color32, f32)>, total: Option<f32>, fmt: &dyn Fn(f32) -> String) {
    resp.on_hover_ui_at_pointer(|ui| {
        ui.label(egui::RichText::new(label).strong().color(IVORY));
        egui::Grid::new("chart_readout").num_columns(2).spacing(egui::vec2(10.0, 1.0)).show(ui, |ui| {
            for (name, col, v) in rows {
                ui.label(egui::RichText::new(name).small().color(col));
                ui.label(egui::RichText::new(fmt(v)).small());
                ui.end_row();
            }
            if let Some(t) = total {
                ui.label(egui::RichText::new("net").small().strong());
                ui.label(egui::RichText::new(fmt(t)).small().strong());
                ui.end_row();
            }
        });
    });
}

/// Lines over a shared x axis. The y axis starts at zero unless a value
/// goes below it. `fmt` formats the hover readout.
pub fn line_chart(ui: &mut egui::Ui, labels: &[String], series: &[Series], height: f32, fmt: &dyn Fn(f32) -> String) {
    let n = labels.len();
    if n == 0 {
        empty(ui, height, "no history yet: the first day closes at midnight");
        return;
    }
    let (resp, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let plot = plot_rect(resp.rect);
    painter.rect_filled(plot, 2.0, PLOT_BG);
    let max = series.iter().flat_map(|s| s.values.iter().copied()).fold(0.0f32, f32::max);
    let min = series.iter().flat_map(|s| s.values.iter().copied()).fold(0.0f32, f32::min);
    let top = nice(max);
    let bottom = if min < 0.0 { -nice(-min) } else { 0.0 };
    let span = (top - bottom).max(1e-6);
    let x_of = |i: usize| if n == 1 { plot.center().x } else { plot.left() + plot.width() * i as f32 / (n - 1) as f32 };
    let y_of = |v: f32| plot.bottom() - plot.height() * (v - bottom) / span;
    if bottom < 0.0 {
        painter.hline(plot.x_range(), y_of(0.0), Stroke::new(1.0, AXIS));
    }
    let small = crate::ui::theme::small();
    painter.text(egui::pos2(plot.left() - 3.0, plot.top()), egui::Align2::RIGHT_TOP, super::fmt::compact(top), small.clone(), TICK);
    painter.text(egui::pos2(plot.left() - 3.0, plot.bottom()), egui::Align2::RIGHT_BOTTOM, super::fmt::compact(bottom), small, TICK);
    x_labels(&painter, plot, labels);
    for s in series {
        let pts: Vec<Pos2> = s.values.iter().take(n).enumerate().map(|(i, v)| egui::pos2(x_of(i), y_of(*v))).collect();
        if pts.len() == 1 {
            painter.circle_filled(pts[0], 2.5, s.color);
        } else {
            painter.line(pts, Stroke::new(1.6, s.color));
        }
    }
    // Lines put their points on the slot edges: snap to the nearest point.
    if let Some(pos) = resp.hover_pos().filter(|p| plot.expand(4.0).contains(*p)) {
        let i = if n == 1 { 0 } else { (((pos.x - plot.left()) / plot.width() * (n - 1) as f32).round().max(0.0) as usize).min(n - 1) };
        painter.vline(x_of(i), plot.y_range(), Stroke::new(1.0, Color32::from_gray(140)));
        let rows = series.iter().filter_map(|s| s.values.get(i).map(|v| (s.name.clone(), s.color, *v))).collect();
        readout(resp, &labels[i], rows, None, fmt);
    }
}

/// Stacked bars, one per x slot: positive values stack up from zero and
/// negative ones down, so income and spending read on one chart; a net
/// line runs through when `net` is set. `overlay` is drawn as a line over
/// the bars on the same scale (demand over the supply stack, say).
pub fn stacked_bars(ui: &mut egui::Ui, labels: &[String], series: &[Series], height: f32, net: bool, overlay: Option<&Series>, fmt: &dyn Fn(f32) -> String) {
    let n = labels.len();
    if n == 0 {
        empty(ui, height, "no history yet: the first day closes at midnight");
        return;
    }
    let (resp, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let plot = plot_rect(resp.rect);
    painter.rect_filled(plot, 2.0, PLOT_BG);
    let at = |s: &Series, i: usize| s.values.get(i).copied().unwrap_or(0.0);
    let (mut up_max, mut down_max) = (0.0f32, 0.0f32);
    for i in 0..n {
        up_max = up_max.max(series.iter().map(|s| at(s, i).max(0.0)).sum::<f32>()).max(overlay.map_or(0.0, |o| at(o, i)));
        down_max = down_max.max(series.iter().map(|s| (-at(s, i)).max(0.0)).sum());
    }
    let top = if up_max > 0.0 { nice(up_max) } else { 0.0 };
    let bottom = if down_max > 0.0 { -nice(down_max) } else { 0.0 };
    let span = (top - bottom).max(1e-6);
    let y_of = |v: f32| plot.bottom() - plot.height() * (v - bottom) / span;
    let bw = plot.width() / n as f32;
    let gap = if bw > 6.0 { 1.0 } else { 0.0 };
    for i in 0..n {
        let x0 = plot.left() + i as f32 * bw + gap;
        let x1 = plot.left() + (i + 1) as f32 * bw - gap;
        let (mut up, mut down) = (0.0f32, 0.0f32);
        for s in series {
            let v = at(s, i);
            if v == 0.0 {
                continue;
            }
            let (a, b) = if v > 0.0 {
                let r = (up, up + v);
                up += v;
                r
            } else {
                let r = (down + v, down);
                down += v;
                r
            };
            let rect = Rect::from_min_max(egui::pos2(x0, y_of(b)), egui::pos2(x1.max(x0 + 1.0), y_of(a)));
            painter.rect_filled(rect, 0.0, s.color);
        }
    }
    painter.hline(plot.x_range(), y_of(0.0), Stroke::new(1.0, AXIS));
    if net && series.len() > 1 {
        let pts: Vec<Pos2> = (0..n).map(|i| egui::pos2(plot.left() + (i as f32 + 0.5) * bw, y_of(series.iter().map(|s| at(s, i)).sum()))).collect();
        if pts.len() > 1 {
            painter.line(pts, Stroke::new(1.5, IVORY));
        }
    }
    if let Some(o) = overlay {
        let pts: Vec<Pos2> = (0..n).map(|i| egui::pos2(plot.left() + (i as f32 + 0.5) * bw, y_of(at(o, i)))).collect();
        if pts.len() > 1 {
            painter.line(pts, Stroke::new(1.6, o.color));
        } else {
            painter.circle_filled(pts[0], 2.5, o.color);
        }
    }
    let small = crate::ui::theme::small();
    painter.text(egui::pos2(plot.left() - 3.0, plot.top()), egui::Align2::RIGHT_TOP, super::fmt::compact(top), small.clone(), TICK);
    painter.text(egui::pos2(plot.left() - 3.0, plot.bottom()), egui::Align2::RIGHT_BOTTOM, super::fmt::compact(bottom), small, TICK);
    x_labels(&painter, plot, labels);
    if let Some(i) = hovered_slot(&resp, plot, n) {
        let x0 = plot.left() + i as f32 * bw;
        painter.rect_stroke(Rect::from_min_max(egui::pos2(x0, plot.top()), egui::pos2(x0 + bw, plot.bottom())), 0.0, Stroke::new(1.0, Color32::from_gray(150)), egui::StrokeKind::Inside);
        let mut rows: Vec<(String, Color32, f32)> = series.iter().filter(|s| at(s, i) != 0.0).map(|s| (s.name.clone(), s.color, at(s, i))).collect();
        if let Some(o) = overlay {
            rows.push((o.name.clone(), o.color, at(o, i)));
        }
        let total = net.then(|| series.iter().map(|s| at(s, i)).sum());
        readout(resp, &labels[i], rows, total, fmt);
    }
}

/// A small chart for a compact view: one line, no axes but the last value.
pub fn sparkline(ui: &mut egui::Ui, values: &[f32], color: Color32, height: f32, tip: &str) {
    let (resp, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 2.0, PLOT_BG);
    if values.len() < 2 {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "the chart fills in day by day", crate::ui::theme::small(), TICK);
        return;
    }
    let max = values.iter().copied().fold(f32::MIN, f32::max);
    let min = values.iter().copied().fold(f32::MAX, f32::min);
    let lo = (min - (max - min) * 0.1).max(0.0);
    let span = (max - lo).max(1.0);
    let inner = rect.shrink2(egui::vec2(4.0, 4.0));
    let pts: Vec<Pos2> = values.iter().enumerate().map(|(i, v)| egui::pos2(inner.left() + inner.width() * i as f32 / (values.len() - 1) as f32, inner.bottom() - inner.height() * (v - lo) / span)).collect();
    painter.line(pts, Stroke::new(1.5, color));
    painter.text(rect.right_top() + egui::vec2(-4.0, 2.0), egui::Align2::RIGHT_TOP, super::fmt::compact(max), crate::ui::theme::small(), TICK);
    resp.on_hover_text(tip);
}

/// Values of a day field over the days.
pub fn col<'a>(days: &[&'a DayRecord], f: impl Fn(&'a DayRecord) -> f32) -> Vec<f32> {
    days.iter().map(|d| f(d)).collect()
}

/// Values of an hour field over the hours.
pub fn hcol<'a>(hours: &[&'a HourRecord], f: impl Fn(&'a HourRecord) -> f32) -> Vec<f32> {
    hours.iter().map(|h| f(h)).collect()
}

/// Fixed colours for up to a dozen series in order (lines, routes).
pub fn palette(i: usize) -> Color32 {
    const P: [Color32; 12] = [
        super::GOLD,
        super::theme::TEAL,
        Color32::from_rgb(210, 95, 70),
        super::theme::GOOD_BRIGHT,
        Color32::from_rgb(190, 130, 230),
        Color32::from_rgb(240, 230, 120),
        Color32::from_rgb(90, 140, 240),
        Color32::from_rgb(240, 130, 180),
        Color32::from_rgb(150, 150, 150),
        Color32::from_rgb(70, 190, 160),
        Color32::from_rgb(230, 150, 60),
        Color32::from_rgb(160, 200, 90),
    ];
    P[i % P.len()]
}
