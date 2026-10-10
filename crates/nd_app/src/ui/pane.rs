//! What the data pages and the tool windows share: the tool windows' header
//! (title and close), the City Data view's page furniture (page title,
//! headline cards, section headings, figure rows, sortable tables), the time-range
//! picker, and the charts.
//!
//! Every City Data page (`ui/city_data.rs`) is laid out the same way, top
//! to bottom: a page title with a line saying what the page is for, a row
//! of headline figures as cards (`kpi`), then sections under one kind of
//! heading (`heading`), each with full-width charts (`line_chart`,
//! `stacked_bars`, `pie`, a `legend` under them), tables (`sortable`) and labelled
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

// ── sortable tables ────────────────────────────────────────────────────────

/// [pt-ux] What a sortable table sorts a column by.
#[derive(Clone, Debug, PartialEq)]
pub enum SortKey {
    /// A figure: the first click sorts biggest first.
    Num(f64),
    /// A name: the first click sorts A to Z (ignoring case).
    Text(String),
    /// Not sortable (a bar, a button), or a blank cell (sinks to the bottom).
    None,
}

impl SortKey {
    pub fn text(s: impl Into<String>) -> Self {
        SortKey::Text(s.into())
    }

    pub fn num(v: impl Into<f64>) -> Self {
        SortKey::Num(v.into())
    }

    fn cmp(&self, o: &SortKey) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (self, o) {
            (SortKey::Num(a), SortKey::Num(b)) => a.total_cmp(b),
            (SortKey::Text(a), SortKey::Text(b)) => a.to_lowercase().cmp(&b.to_lowercase()),
            // Mixed: figures before names, blanks last.
            (SortKey::Num(_), _) => Ordering::Less,
            (_, SortKey::Num(_)) => Ordering::Greater,
            (SortKey::Text(_), SortKey::None) => Ordering::Less,
            (SortKey::None, SortKey::Text(_)) => Ordering::Greater,
            (SortKey::None, SortKey::None) => Ordering::Equal,
        }
    }
}

/// A table's sort: the column and the direction (`col: None`: as given).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Sort {
    pub col: Option<usize>,
    pub desc: bool,
}

impl Sort {
    /// A click on column `c`'s heading, whose first key is `first`: a new
    /// column sorts figures biggest first and names A to Z; the same column
    /// again reverses; a third click puts the given order back.
    pub fn click(self, c: usize, first: &SortKey) -> Sort {
        let numeric = matches!(first, SortKey::Num(_));
        match self.col {
            Some(o) if o == c => {
                if self.desc != numeric {
                    Sort::default()
                } else {
                    Sort { col: Some(c), desc: !self.desc }
                }
            }
            _ => Sort { col: Some(c), desc: numeric },
        }
    }
}

/// The order rows show in under `sort` (stable: equal keys keep the given
/// order; blanks sink to the bottom either way).
pub fn sort_order<R>(rows: &[R], sort: Sort, key: impl Fn(&R, usize) -> SortKey) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    let Some(c) = sort.col else { return order };
    let keys: Vec<SortKey> = rows.iter().map(|r| key(r, c)).collect();
    order.sort_by(|&a, &b| {
        let (ka, kb) = (&keys[a], &keys[b]);
        match (ka, kb) {
            (SortKey::None, SortKey::None) => std::cmp::Ordering::Equal,
            (SortKey::None, _) => std::cmp::Ordering::Greater,
            (_, SortKey::None) => std::cmp::Ordering::Less,
            _ if sort.desc => kb.cmp(ka),
            _ => ka.cmp(kb),
        }
    });
    order
}

fn sort_id(id: egui::Id) -> egui::Id {
    id.with("pt_ux_sort")
}

/// Set a table's sort (the API's `city_data {sort}`).
pub fn set_sort(ctx: &egui::Context, id: impl std::hash::Hash + std::fmt::Debug, sort: Sort) {
    ctx.data_mut(|d| d.insert_temp(sort_id(egui::Id::new(id)), sort));
}

/// [pt-ux] A table like [`table`] whose headings sort it: click one to sort
/// by it (figures biggest first, names A to Z), again to reverse, a third
/// time for the given order. `heads` are (label, hover tip); `key` gives a
/// row's sort key in a column (`SortKey::None` where a column is not
/// sortable); `row` draws one row's cells (the table ends the row). The
/// sort lasts the session, in egui memory under `id`.
pub fn sortable<R>(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, heads: &[(&str, &str)], rows: &[R], key: impl Fn(&R, usize) -> SortKey, row: impl FnMut(&mut egui::Ui, &R)) {
    sortable_foot(ui, id, heads, rows, key, row, |_| {});
}

/// [`sortable`] with a last row that stays last (a total): `foot` adds its
/// cells.
pub fn sortable_foot<R>(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, heads: &[(&str, &str)], rows: &[R], key: impl Fn(&R, usize) -> SortKey, mut row: impl FnMut(&mut egui::Ui, &R), foot: impl FnOnce(&mut egui::Ui)) {
    let id = egui::Id::new(id);
    let mut sort: Sort = ui.data(|d| d.get_temp(sort_id(id))).unwrap_or_default();
    let order = sort_order(rows, sort, &key);
    egui::Grid::new(id).num_columns(heads.len().max(1)).spacing(egui::vec2(16.0, 4.0)).striped(true).show(ui, |ui| {
        for (c, (h, tip)) in heads.iter().enumerate() {
            let first = rows.iter().map(|r| key(r, c)).find(|k| *k != SortKey::None).unwrap_or(SortKey::None);
            if h.is_empty() || first == SortKey::None {
                let l = th(ui, h);
                if !tip.is_empty() {
                    l.on_hover_text(*tip);
                }
                continue;
            }
            let on = sort.col == Some(c);
            let arrow = if on { if sort.desc { " ▼" } else { " ▲" } } else { "" };
            let text = egui::RichText::new(format!("{h}{arrow}")).small().strong().color(if on { GOLD } else { CAPTION });
            let r = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
            if r.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let clicked = r.clicked();
            r.on_hover_text(if tip.is_empty() { "Click to sort; again to reverse".to_string() } else { format!("{tip}. Click to sort; again to reverse") });
            if clicked {
                sort = sort.click(c, &first);
            }
        }
        ui.end_row();
        for &i in &order {
            row(ui, &rows[i]);
            ui.end_row();
        }
        foot(ui);
    });
    ui.data_mut(|d| d.insert_temp(sort_id(id), sort));
}

/// [`sortable`] with plain headings.
pub fn sortable_plain<R>(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, heads: &[&str], rows: &[R], key: impl Fn(&R, usize) -> SortKey, row: impl FnMut(&mut egui::Ui, &R)) {
    let heads: Vec<(&str, &str)> = heads.iter().map(|h| (*h, "")).collect();
    sortable(ui, id, &heads, rows, key, row);
}

// ── pie ────────────────────────────────────────────────────────────────────

/// [pt-ux] Each slice's start and end angle (radians, clockwise from
/// twelve o'clock), `None` for a value not over zero.
pub fn pie_angles(values: &[f32]) -> Vec<Option<(f32, f32)>> {
    let total: f32 = values.iter().filter(|v| **v > 0.0).sum();
    let mut at = 0.0;
    values
        .iter()
        .map(|&v| {
            if v <= 0.0 || total <= 0.0 {
                return None;
            }
            let span = v / total * std::f32::consts::TAU;
            let s = (at, at + span);
            at += span;
            Some(s)
        })
        .collect()
}

/// [pt-ux] A pie chart of `slices` (label, value, colour) with its key
/// beside it: each slice's value (`fmt`) and share. Hovering a slice lights
/// it and its key line. Nothing over zero: a line saying so.
pub fn pie(ui: &mut egui::Ui, title: &str, slices: &[(String, f32, Color32)], radius: f32, fmt: &dyn Fn(f32) -> String) {
    let values: Vec<f32> = slices.iter().map(|s| s.1).collect();
    let total: f32 = values.iter().filter(|v| **v > 0.0).sum();
    let angles = pie_angles(&values);
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(title).small().strong().color(CAPTION));
        if total <= 0.0 {
            ui.label(egui::RichText::new("Nothing yet").weak());
            return;
        }
        ui.horizontal(|ui| {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(radius * 2.0 + 8.0, radius * 2.0 + 8.0), egui::Sense::hover());
            let c = rect.center();
            let hover = resp.hover_pos().and_then(|p| {
                let d = p - c;
                if d.length() > radius {
                    return None;
                }
                let mut a = d.x.atan2(-d.y);
                if a < 0.0 {
                    a += std::f32::consts::TAU;
                }
                angles.iter().position(|s| s.is_some_and(|(a0, a1)| a >= a0 && a < a1))
            });
            let painter = ui.painter();
            for (i, s) in angles.iter().enumerate() {
                let Some((a0, a1)) = *s else { continue };
                let r = if hover == Some(i) { radius + 3.0 } else { radius };
                // One convex polygon per half at most (a slice over half the
                // pie is not convex): no seams inside a slice.
                let halves = if a1 - a0 > std::f32::consts::PI { 2 } else { 1 };
                for h in 0..halves {
                    let (b0, b1) = (a0 + (a1 - a0) * h as f32 / halves as f32, a0 + (a1 - a0) * (h + 1) as f32 / halves as f32);
                    let steps = ((b1 - b0) / 0.05).ceil().max(1.0) as usize;
                    let mut pts = vec![c];
                    for k in 0..=steps {
                        let t = b0 + (b1 - b0) * k as f32 / steps as f32;
                        pts.push(c + egui::vec2(t.sin(), -t.cos()) * r);
                    }
                    painter.add(egui::Shape::convex_polygon(pts, slices[i].2, Stroke::NONE));
                }
            }
            // Thin dark edges between the slices.
            if angles.iter().flatten().count() > 1 {
                for (a0, _) in angles.iter().flatten() {
                    painter.line_segment([c, c + egui::vec2(a0.sin(), -a0.cos()) * (radius + 3.0)], Stroke::new(1.5, PLOT_BG));
                }
            }
            if let Some(i) = hover {
                resp.on_hover_text(format!("{}: {} ({:.0}%)", slices[i].0, fmt(slices[i].1), slices[i].1 / total * 100.0));
            }
            ui.add_space(8.0);
            ui.vertical(|ui| {
                for (i, (label, v, col)) in slices.iter().enumerate() {
                    if *v <= 0.0 {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        let (sw, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().rect_filled(sw, 2.0, *col);
                        let text = format!("{label}  {}  {:.0}%", fmt(*v), v / total * 100.0);
                        ui.label(if hover == Some(i) { egui::RichText::new(text).small().color(IVORY).strong() } else { egui::RichText::new(text).small() });
                    });
                }
            });
        });
    });
}

#[cfg(test)]
mod sort_tests {
    use super::*;

    fn key(r: &(&str, f64), c: usize) -> SortKey {
        match c {
            0 => SortKey::text(r.0),
            1 if r.1 < 0.0 => SortKey::None,
            1 => SortKey::num(r.1),
            _ => SortKey::None,
        }
    }

    #[test]
    fn sorting_by_figures_names_and_blanks() {
        let rows = [("bakery", 3.0), ("Arcade", 9.0), ("clinic", -1.0), ("dock", 3.0)];
        assert_eq!(sort_order(&rows, Sort::default(), key), vec![0, 1, 2, 3]);
        // Names A to Z, ignoring case.
        assert_eq!(sort_order(&rows, Sort { col: Some(0), desc: false }, key), vec![1, 0, 2, 3]);
        assert_eq!(sort_order(&rows, Sort { col: Some(0), desc: true }, key), vec![3, 2, 0, 1]);
        // Figures biggest first; ties keep their order; the blank sinks either way.
        assert_eq!(sort_order(&rows, Sort { col: Some(1), desc: true }, key), vec![1, 0, 3, 2]);
        assert_eq!(sort_order(&rows, Sort { col: Some(1), desc: false }, key), vec![0, 3, 1, 2]);
    }

    #[test]
    fn heading_clicks_cycle() {
        let n = SortKey::num(1.0);
        let t = SortKey::text("a");
        let s = Sort::default().click(1, &n);
        assert_eq!(s, Sort { col: Some(1), desc: true });
        let s = s.click(1, &n);
        assert_eq!(s, Sort { col: Some(1), desc: false });
        assert_eq!(s.click(1, &n), Sort::default());
        let s = Sort::default().click(0, &t);
        assert_eq!(s, Sort { col: Some(0), desc: false });
        assert_eq!(s.click(0, &t), Sort { col: Some(0), desc: true });
        assert_eq!(s.click(0, &t).click(0, &t), Sort::default());
        // Another column starts afresh.
        assert_eq!(s.click(1, &n), Sort { col: Some(1), desc: true });
    }

    #[test]
    fn pie_slices_cover_the_circle() {
        let a = pie_angles(&[1.0, 0.0, 3.0, -2.0]);
        assert_eq!(a[1], None);
        assert_eq!(a[3], None);
        let (s0, e0) = a[0].unwrap();
        let (s2, e2) = a[2].unwrap();
        assert_eq!(s0, 0.0);
        assert!((e0 - std::f32::consts::TAU / 4.0).abs() < 1e-5);
        assert_eq!(e0, s2);
        assert!((e2 - std::f32::consts::TAU).abs() < 1e-4);
        assert!(pie_angles(&[0.0, 0.0]).iter().all(Option::is_none));
    }
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

/// A small rounded tag in a colour: the inspector's header (zone, business,
/// district) and a train's kind.
pub fn tag(ui: &mut egui::Ui, text: &str, col: Color32) {
    egui::Frame::new().fill(col.gamma_multiply(0.22)).stroke(Stroke::new(1.0, col.gamma_multiply(0.8))).corner_radius(4.0).inner_margin(egui::Margin::symmetric(6, 1)).show(ui, |ui| {
        ui.label(egui::RichText::new(text).color(col).strong());
    });
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
