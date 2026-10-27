//! [directory] City Data's Directory tab: every business, every company
//! (owner) and every person in one sortable table each (Businesses,
//! Companies, People), each with a search box.
//!
//! Cities run to ten thousand people and more, so the tables never lay out
//! what is off screen: the body is an `egui::ScrollArea::show_rows` that
//! paints only the rows in view, each row painted by hand (no widgets but
//! the "go" button), and the header stays put above it. The rows come from
//! `crate::directory`: a snapshot of the world rebuilt every few seconds
//! while the page shows (not every frame), and the sorted, filtered order
//! is cached beside it and rebuilt only when the sort, the filter or the
//! snapshot changes. Both live in egui memory with the page's settings
//! (sort column and direction, search, pickers), so they last the session.
//!
//! A click on a row selects the business or the person (the inspector opens
//! over the page); "go" (or a double click) flies there and shows the map.

use std::sync::Arc;

use bevy_egui::egui::{self, Color32};
use nd_sim::estate::Class;

use super::pane::{self, CAPTION};
use super::fmt::capitalise;
use super::{UiCtx, GOLD, IVORY, WARN};
use crate::directory::{self as dir, BizCol, BizRow, BizState, CompanyCol, CompanyRow, Filter, PersonCol, PersonRow, Row, Status};
use crate::tools::Selection;

/// Seconds between snapshots of the world while the page shows.
const REFRESH: f64 = 5.0;
/// A row's height.
const ROW_H: f32 = 22.0;
/// The "go" column at the right.
const GO_W: f32 = 34.0;

const GOOD: Color32 = super::theme::GOOD_LIGHT;
const QUIET: Color32 = Color32::from_gray(150);

// ── settings and caches, in egui memory ───────────────────────────────────

/// A table's settings: the filter and the sort.
#[derive(Clone, Debug, PartialEq)]
pub struct TableState<C> {
    pub filter: Filter,
    pub col: C,
    pub desc: bool,
}

impl Default for TableState<BizCol> {
    fn default() -> Self {
        Self { filter: Filter::default(), col: BizCol::Name, desc: false }
    }
}

impl Default for TableState<CompanyCol> {
    fn default() -> Self {
        Self { filter: Filter::default(), col: CompanyCol::Value, desc: true }
    }
}

impl Default for TableState<PersonCol> {
    fn default() -> Self {
        Self { filter: Filter::default(), col: PersonCol::Name, desc: false }
    }
}

/// The rows as last read from the world, and the order shown.
#[derive(Clone)]
struct Cache<R: Row> {
    rows: Arc<Vec<R>>,
    /// egui time and world tick of the snapshot.
    at: f64,
    tick: u64,
    /// The city the snapshot is of (a new game or a load replaces it).
    generation: u64,
    order: Arc<Vec<u32>>,
    /// The settings `order` was made for.
    made_for: Option<TableState<R::Col>>,
    fresh: bool,
}

impl<R: Row> Default for Cache<R> {
    fn default() -> Self {
        Self { rows: Arc::new(Vec::new()), at: f64::NEG_INFINITY, tick: u64::MAX, generation: u64::MAX, order: Arc::new(Vec::new()), made_for: None, fresh: false }
    }
}

fn state_id(which: &str) -> egui::Id {
    egui::Id::new(("directory_state", which))
}

fn cache_id(which: &str) -> egui::Id {
    egui::Id::new(("directory_cache", which))
}

fn state<C>(ctx: &egui::Context, which: &str) -> TableState<C>
where
    TableState<C>: Default + Clone + Send + Sync + 'static,
{
    ctx.data(|d| d.get_temp::<TableState<C>>(state_id(which))).unwrap_or_default()
}

fn save_state<C>(ctx: &egui::Context, which: &str, s: TableState<C>)
where
    TableState<C>: Clone + Send + Sync + 'static,
{
    ctx.data_mut(|d| d.insert_temp(state_id(which), s));
}

/// [remote] Set a table's settings from the API (`directory {show}`).
pub fn set_businesses(ctx: &egui::Context, s: TableState<BizCol>) {
    save_state(ctx, "businesses", s);
}

pub fn set_people(ctx: &egui::Context, s: TableState<PersonCol>) {
    save_state(ctx, "people", s);
}

pub fn set_companies(ctx: &egui::Context, s: TableState<CompanyCol>) {
    save_state(ctx, "companies", s);
}

/// The snapshot and the order for `st`: a new snapshot when the last is
/// `REFRESH` seconds old and the world has moved on (or is another city),
/// a new order when the snapshot or the settings changed.
fn cached<R>(ctx: &egui::Context, which: &str, now: f64, tick: u64, generation: u64, st: &TableState<R::Col>, read: impl FnOnce() -> Vec<R>) -> (Arc<Vec<R>>, Arc<Vec<u32>>)
where
    R: Row + Clone + Send + Sync + 'static,
    R::Col: std::fmt::Debug + Send + Sync,
{
    ctx.data_mut(|d| {
        let c = d.get_temp_mut_or_default::<Cache<R>>(cache_id(which));
        let stale = generation != c.generation || (now - c.at >= REFRESH && tick != c.tick) || now < c.at;
        if stale {
            c.rows = Arc::new(read());
            c.at = now;
            c.tick = tick;
            c.generation = generation;
            c.fresh = true;
        }
        if c.fresh || c.made_for.as_ref() != Some(st) {
            c.order = Arc::new(dir::order(&c.rows, &st.filter, st.col, st.desc));
            c.made_for = Some(st.clone());
            c.fresh = false;
        }
        (c.rows.clone(), c.order.clone())
    })
}

// ── the pages ──────────────────────────────────────────────────────────────

/// What a click in a table asked for.
enum Act {
    Select(u32),
    Go(u32),
}

/// City Data, Directory, Businesses.
pub fn businesses(ui: &mut egui::Ui, p: &mut UiCtx) {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let mut st: TableState<BizCol> = state(&ctx, "businesses");
    let w = &p.sim.world;
    let (rows, order) = cached(&ctx, "businesses", now, w.clock.tick, p.sim.generation, &st, || dir::businesses(w));

    // Headline figures over every business.
    let count = |s: BizState| rows.iter().filter(|r| r.state == s).count();
    let staff: usize = rows.iter().map(|r| r.staff as usize).sum();
    let jobs: usize = rows.iter().map(|r| r.jobs as usize).sum();
    let takings: f32 = rows.iter().map(|r| r.revenue_today).sum();
    pane::cards(ui, |ui| {
        pane::kpi(ui, "businesses", egui::RichText::new(format!("{}", rows.len())).color(GOLD));
        pane::kpi(ui, "open now", egui::RichText::new(format!("{}", count(BizState::Open))).color(GOOD));
        pane::kpi(ui, "broke or out of business", egui::RichText::new(format!("{}", count(BizState::Broke) + count(BizState::Shut))).color(if count(BizState::Broke) + count(BizState::Shut) > 0 { WARN } else { IVORY }));
        pane::kpi(ui, "under construction", egui::RichText::new(format!("{}", count(BizState::Building))));
        pane::kpi(ui, "staff of jobs", egui::RichText::new(format!("{} / {}", super::fmt::compact(staff as f32), super::fmt::compact(jobs as f32))));
        pane::kpi(ui, "takings today", egui::RichText::new(super::fmt::compact(takings)).color(GOOD));
    });
    ui.add_space(8.0);

    // The search and the pickers.
    let before = st.clone();
    ui.horizontal_wrapped(|ui| {
        search_box(ui, &mut st.filter.search, "search names, kinds, owners, streets");
        egui::ComboBox::from_id_salt("directory_biz_kind")
            .selected_text(st.filter.kind.map_or("every kind".to_string(), |k| capitalise(k.label())))
            .height(420.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut st.filter.kind, None, "every kind");
                let mut kinds: Vec<nd_sim::BusinessKind> = rows.iter().map(|r| r.kind).collect();
                kinds.sort_by_key(|k| k.label());
                kinds.dedup();
                for k in kinds {
                    let n = rows.iter().filter(|r| r.kind == k).count();
                    ui.selectable_value(&mut st.filter.kind, Some(k), format!("{} ({n})", capitalise(k.label())));
                }
            });
        district_picker(ui, "directory_biz_district", &mut st.filter.district, rows.iter().map(|r| r.district.as_str()));
        egui::ComboBox::from_id_salt("directory_biz_state").selected_text(st.filter.state.map_or("any state", |s| s.label())).show_ui(ui, |ui| {
            ui.selectable_value(&mut st.filter.state, None, "any state");
            for s in BizState::ALL {
                ui.selectable_value(&mut st.filter.state, Some(s), egui::RichText::new(s.label()).color(state_color(s)));
            }
        });
        clear_button(ui, &mut st.filter);
        count_line(ui, order.len(), rows.len(), "business", "businesses");
    });
    ui.add_space(4.0);

    let sel = match *p.selection {
        Selection::Building(id) => Some(id),
        _ => None,
    };
    let cols = &BizCol::ALL;
    let weights = [2.1, 1.5, 1.4, 1.0, 1.3, 0.7, 0.8, 0.8, 0.8, 0.8, 0.7, 0.75, 1.45];
    let act = table(ui, "directory_biz", cols, &weights, &mut st.col, &mut st.desc, |c| (c.label(), c.tip(), c.numeric()), &rows, &order, |r| sel == Some(r.id), biz_cell);
    if st != before {
        save_state(&ctx, "businesses", st);
    }
    match act {
        Some(Act::Select(i)) => *p.selection = Selection::Building(rows[i as usize].id),
        Some(Act::Go(i)) => {
            let r = &rows[i as usize];
            *p.selection = Selection::Building(r.id);
            p.fly_to = r.pos;
        }
        None => {}
    }
}

/// [companies] City Data, Directory, Companies: every owner, biggest first.
pub fn companies(ui: &mut egui::Ui, p: &mut UiCtx) {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let mut st: TableState<CompanyCol> = state(&ctx, "companies");
    let w = &p.sim.world;
    let (rows, order) = cached(&ctx, "companies", now, w.clock.tick, p.sim.generation, &st, || dir::companies(w));

    let count = |k: &str| rows.iter().filter(|r| r.kind_key == k).count();
    let local = rows.iter().filter(|r| matches!(r.kind_key, "builder" | "firm" | "founder" | "property" | "corp") && r.properties > 0).count();
    let fleets = rows.iter().filter(|r| r.vehicles > 0).count();
    let vehicles: u32 = rows.iter().map(|r| r.vehicles).sum();
    pane::cards(ui, |ui| {
        pane::kpi(ui, "owners", egui::RichText::new(format!("{}", rows.len())).color(GOLD));
        pane::kpi(ui, "local companies", egui::RichText::new(format!("{local}")).color(GOOD));
        pane::kpi(ui, "builders", egui::RichText::new(format!("{}", count("builder"))));
        pane::kpi(ui, "mega-corps", egui::RichText::new(format!("{}", count("corp"))));
        pane::kpi(ui, "with a fleet", egui::RichText::new(format!("{fleets} ({vehicles} vehicles)")));
        if count("developer") > 0 {
            pane::kpi(ui, "old developers selling up", egui::RichText::new(format!("{}", count("developer"))).color(WARN));
        }
    });
    ui.add_space(8.0);

    let before = st.clone();
    ui.horizontal_wrapped(|ui| {
        search_box(ui, &mut st.filter.search, "search names, founders, kinds, history");
        egui::ComboBox::from_id_salt("directory_company_kind").selected_text(st.filter.owner_kind.and_then(|k| dir::OWNER_KINDS.iter().find(|(x, _)| *x == k)).map_or("every kind of owner", |(_, w)| *w)).show_ui(ui, |ui| {
            ui.selectable_value(&mut st.filter.owner_kind, None, "every kind of owner");
            for (k, words) in dir::OWNER_KINDS {
                let n = count(k);
                if n > 0 {
                    ui.selectable_value(&mut st.filter.owner_kind, Some(k), format!("{words} ({n})"));
                }
            }
        });
        district_picker(ui, "directory_company_district", &mut st.filter.district, rows.iter().map(|r| r.district.as_str()));
        clear_button(ui, &mut st.filter);
        count_line(ui, order.len(), rows.len(), "owner", "owners");
    });
    ui.add_space(4.0);

    let sel_owner = match *p.selection {
        Selection::Building(id) => p.sim.world.buildings.get(id).and_then(|b| b.plot.owner),
        _ => None,
    };
    let cols = &CompanyCol::ALL;
    let weights = [1.9, 2.0, 1.2, 0.55, 0.8, 0.75, 0.85, 0.6, 0.7, 0.75, 2.1];
    let act = table(ui, "directory_companies", cols, &weights, &mut st.col, &mut st.desc, |c| (c.label(), c.tip(), c.numeric()), &rows, &order, |r| sel_owner == Some(r.owner), company_cell);
    if st != before {
        save_state(&ctx, "companies", st);
    }
    match act {
        Some(Act::Select(i)) => {
            if let Some(b) = rows[i as usize].flagship {
                *p.selection = Selection::Building(b);
            }
        }
        Some(Act::Go(i)) => {
            let r = &rows[i as usize];
            if let Some(b) = r.flagship {
                *p.selection = Selection::Building(b);
            }
            p.fly_to = r.pos;
        }
        None => {}
    }
}

/// City Data, Directory, People.
pub fn people(ui: &mut egui::Ui, p: &mut UiCtx) {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let mut st: TableState<PersonCol> = state(&ctx, "people");
    let w = &p.sim.world;
    let (rows, order) = cached(&ctx, "people", now, w.clock.tick, p.sim.generation, &st, || dir::people(w));

    let adults = rows.iter().filter(|r| r.stage == nd_sim::citizens::LifeStage::Adult);
    let (mut working, mut jobless) = (0, 0);
    for r in adults {
        if r.work.is_some() {
            working += 1;
        } else {
            jobless += 1;
        }
    }
    let homeless = rows.iter().filter(|r| r.home.is_none()).count();
    let ill = rows.iter().filter(|r| r.ill).count();
    let mood = if rows.is_empty() { 0.0 } else { rows.iter().map(|r| r.mood).sum::<f32>() / rows.len() as f32 };
    pane::cards(ui, |ui| {
        pane::kpi(ui, "people", egui::RichText::new(super::fmt::compact(rows.len() as f32)).color(GOLD));
        pane::kpi(ui, "adults in work", egui::RichText::new(super::fmt::compact(working as f32)).color(GOOD));
        pane::kpi(ui, "adults out of work", egui::RichText::new(super::fmt::compact(jobless as f32)).color(if jobless > 0 { WARN } else { IVORY }));
        pane::kpi(ui, "without a home", egui::RichText::new(format!("{homeless}")).color(if homeless > 0 { WARN } else { IVORY }));
        pane::kpi(ui, "ill", egui::RichText::new(format!("{ill}")));
        pane::kpi(ui, "average mood", egui::RichText::new(format!("{:.0}%", mood * 100.0)).color(mood_color(mood)));
    });
    ui.add_space(8.0);

    let before = st.clone();
    ui.horizontal_wrapped(|ui| {
        search_box(ui, &mut st.filter.search, "search names, jobs, homes, streets");
        egui::ComboBox::from_id_salt("directory_people_class").selected_text(st.filter.class.map_or("every class", class_label)).show_ui(ui, |ui| {
            ui.selectable_value(&mut st.filter.class, None, "every class");
            for c in [Class::Low, Class::Middle, Class::High, Class::Elite] {
                let n = rows.iter().filter(|r| r.class == c).count();
                ui.selectable_value(&mut st.filter.class, Some(c), format!("{} ({n})", class_label(c)));
            }
        });
        district_picker(ui, "directory_people_district", &mut st.filter.district, rows.iter().map(|r| r.district.as_str()));
        egui::ComboBox::from_id_salt("directory_people_status").selected_text(st.filter.status.map_or("doing anything", |s| s.label())).show_ui(ui, |ui| {
            ui.selectable_value(&mut st.filter.status, None, "doing anything");
            for s in Status::ALL {
                let n = rows.iter().filter(|r| r.status == s).count();
                ui.selectable_value(&mut st.filter.status, Some(s), egui::RichText::new(format!("{} ({n})", s.label())).color(status_color(s)));
            }
        });
        clear_button(ui, &mut st.filter);
        count_line(ui, order.len(), rows.len(), "person", "people");
    });
    ui.add_space(4.0);

    let sel = match *p.selection {
        Selection::Citizen(id) => Some(id),
        _ => None,
    };
    let cols = &PersonCol::ALL;
    let weights = [1.7, 0.45, 0.7, 0.65, 1.6, 1.8, 0.4, 0.6, 0.9, 0.65, 1.4, 0.55, 1.1];
    let act = table(ui, "directory_people", cols, &weights, &mut st.col, &mut st.desc, |c| (c.label(), c.tip(), c.numeric()), &rows, &order, |r| sel == Some(r.id), person_cell);
    if st != before {
        save_state(&ctx, "people", st);
    }
    match act {
        Some(Act::Select(i)) => *p.selection = Selection::Citizen(rows[i as usize].id),
        Some(Act::Go(i)) => {
            let r = &rows[i as usize];
            *p.selection = Selection::Citizen(r.id);
            // Where they are now, not where the snapshot saw them.
            p.fly_to = Some(p.sim.world.citizens.get(r.id).map_or(r.pos, |c| c.pos));
        }
        None => {}
    }
}

// ── the furniture above the tables ─────────────────────────────────────────

fn search_box(ui: &mut egui::Ui, text: &mut String, hint: &str) {
    ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(270.0));
}

fn district_picker<'a>(ui: &mut egui::Ui, salt: &str, pick: &mut Option<String>, names: impl Iterator<Item = &'a str>) {
    let shown = pick.clone().unwrap_or_else(|| "every district".into());
    egui::ComboBox::from_id_salt(salt).selected_text(shown).show_ui(ui, |ui| {
        ui.selectable_value(pick, None, "every district");
        for d in dir::districts_in(names) {
            let v = Some(d.clone());
            ui.selectable_value(pick, v, d);
        }
    });
}

fn clear_button(ui: &mut egui::Ui, f: &mut Filter) {
    if *f != Filter::default() && ui.small_button("clear").on_hover_text("Clear the search and the pickers").clicked() {
        *f = Filter::default();
    }
}

fn count_line(ui: &mut egui::Ui, shown: usize, all: usize, one: &str, many: &str) {
    let text = if shown == all { format!("{all} {}", if all == 1 { one } else { many }) } else { format!("{shown} of {all} {many}") };
    ui.label(egui::RichText::new(text).color(CAPTION)).on_hover_text("Read from the city every few seconds while this page is open");
}

// ── the table ──────────────────────────────────────────────────────────────

/// Draw a sortable table of `order`'s rows: a header that sorts (click a
/// heading; again to reverse it), then the rows in a scroll area that lays
/// out only the ones in view. `head` gives a column's label, tip and
/// whether it is a number (right-aligned, biggest first on the first
/// click); `cell` a cell's text and colour.
#[allow(clippy::too_many_arguments)]
fn table<R: Row>(
    ui: &mut egui::Ui,
    salt: &str,
    cols: &[R::Col],
    weights: &[f32],
    sort: &mut R::Col,
    desc: &mut bool,
    head: impl Fn(R::Col) -> (&'static str, &'static str, bool),
    rows: &[R],
    order: &[u32],
    selected: impl Fn(&R) -> bool,
    cell: impl Fn(&R, R::Col) -> (String, Color32),
) -> Option<Act> {
    let width = ui.available_width();
    let total: f32 = weights.iter().sum();
    let widths: Vec<f32> = weights.iter().map(|w| (width - GO_W) * w / total).collect();
    let font = crate::ui::theme::body();
    let small = crate::ui::theme::small();

    // The header.
    let (head_rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_H + 2.0), egui::Sense::hover());
    let mut x = head_rect.left();
    for (i, &c) in cols.iter().enumerate() {
        let (label, tip, numeric) = head(c);
        let rect = egui::Rect::from_min_size(egui::pos2(x, head_rect.top()), egui::vec2(widths[i], head_rect.height()));
        x += widths[i];
        let r = ui.interact(rect, egui::Id::new((salt, "head", i)), egui::Sense::click()).on_hover_text(tip);
        let on = *sort == c;
        if r.clicked() {
            if on {
                *desc = !*desc;
            } else {
                *sort = c;
                *desc = numeric;
            }
        }
        let color = if on { GOLD } else if r.hovered() { IVORY } else { CAPTION };
        let galley = ui.painter().layout_no_wrap(label.to_string(), small.clone(), color);
        let tx = if numeric { rect.right() - 6.0 - galley.size().x } else { rect.left() + 4.0 };
        let ty = rect.center().y - galley.size().y / 2.0;
        // The arrow: before a right-aligned heading, after a left-aligned one.
        let ax = if numeric { tx - 9.0 } else { tx + galley.size().x + 7.0 };
        ui.painter().galley(egui::pos2(tx, ty), galley, color);
        if on {
            arrow(ui.painter(), egui::pos2(ax, rect.center().y), *desc, GOLD);
        }
    }
    ui.painter().hline(head_rect.x_range(), head_rect.bottom(), egui::Stroke::new(1.0, super::theme::EDGE));

    if order.is_empty() {
        ui.add_space(10.0);
        ui.label(egui::RichText::new(if rows.is_empty() { "Nobody here yet." } else { "Nothing matches: clear the search or the pickers." }).color(CAPTION));
        return None;
    }

    // The body: only the rows in view are laid out.
    // Inside the page's own scroll area the space left is unbounded, so the
    // table takes the rest of the view: its header stays put above it.
    let height = (ui.clip_rect().bottom() - ui.cursor().top() - 34.0).max(220.0);
    let mut act = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::ScrollArea::vertical().id_salt((salt, "rows")).min_scrolled_height(height).max_height(height).auto_shrink([false, true]).show_rows(ui, ROW_H, order.len(), |ui, range| {
            for k in range {
                let i = order[k];
                let row = &rows[i as usize];
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::click());
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let p = ui.painter();
                let on = selected(row);
                let fill = if on {
                    Some(Color32::from_rgb(70, 52, 24))
                } else if resp.hovered() {
                    Some(Color32::from_rgb(38, 34, 30))
                } else if k % 2 == 1 {
                    Some(Color32::from_rgb(16, 19, 30))
                } else {
                    None
                };
                if let Some(f) = fill {
                    p.rect_filled(rect, 0.0, f);
                }
                let mut x = rect.left();
                for (j, &c) in cols.iter().enumerate() {
                    let w = widths[j];
                    let (text, color) = cell(row, c);
                    if !text.is_empty() {
                        let numeric = head(c).2;
                        let galley = egui::WidgetText::from(egui::RichText::new(text).font(font.clone()).color(color)).into_galley(ui, Some(egui::TextWrapMode::Truncate), (w - 10.0).max(8.0), egui::TextStyle::Body);
                        let tx = if numeric { x + w - 6.0 - galley.size().x } else { x + 4.0 };
                        p.galley(egui::pos2(tx, rect.center().y - galley.size().y / 2.0), galley, color);
                    }
                    x += w;
                }
                let go = egui::Rect::from_min_size(egui::pos2(rect.right() - GO_W + 2.0, rect.top() + 1.0), egui::vec2(GO_W - 4.0, ROW_H - 2.0));
                if ui.put(go, egui::Button::new(egui::RichText::new("go").small()).small()).on_hover_text("Fly there").clicked() {
                    act = Some(Act::Go(i));
                } else if resp.double_clicked() {
                    act = Some(Act::Go(i));
                } else if resp.clicked() {
                    act = Some(Act::Select(i));
                }
            }
        });
    });
    act
}

/// A small painted triangle: up for ascending, down for descending (the
/// fonts are not relied on for arrows).
fn arrow(p: &egui::Painter, at: egui::Pos2, down: bool, color: Color32) {
    let (s, h) = (4.0, 3.5);
    let pts = if down { vec![at + egui::vec2(-s, -h / 2.0), at + egui::vec2(s, -h / 2.0), at + egui::vec2(0.0, h)] } else { vec![at + egui::vec2(-s, h / 2.0), at + egui::vec2(s, h / 2.0), at + egui::vec2(0.0, -h)] };
    p.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

// ── cells ──────────────────────────────────────────────────────────────────

fn signed(v: f32) -> (String, Color32) {
    (super::fmt::signed(v), if v < 0.0 { WARN } else if v > 0.0 { GOOD } else { IVORY })
}

fn state_color(s: BizState) -> Color32 {
    match s {
        BizState::Open => GOOD,
        BizState::AfterHours => QUIET,
        BizState::NoStaff => super::theme::AMBER,
        BizState::Broke => WARN,
        BizState::Shut => Color32::from_rgb(200, 90, 80),
        BizState::Building => Color32::from_rgb(200, 150, 60),
    }
}

fn status_color(s: Status) -> Color32 {
    match s {
        Status::Working => super::theme::SERVICE_BLUE,
        Status::AtHome => IVORY,
        Status::Asleep => QUIET,
        Status::Out => Color32::from_rgb(220, 150, 255),
        Status::Travelling => Color32::from_rgb(150, 220, 200),
        Status::Ill => WARN,
        Status::Homeless => super::theme::AMBER,
        Status::Arriving | Status::Away => QUIET,
    }
}

fn class_label(c: Class) -> &'static str {
    match c {
        Class::Low => "low income",
        Class::Middle => "middle",
        Class::High => "high income",
        Class::Elite => "super-elite",
    }
}

fn class_color(c: Class) -> Color32 {
    match c {
        Class::Low => Color32::from_rgb(200, 170, 130),
        Class::Middle => IVORY,
        Class::High => Color32::from_rgb(140, 210, 255),
        Class::Elite => GOLD,
    }
}

/// Red through amber to green.
fn mood_color(m: f32) -> Color32 {
    if m < 0.35 {
        WARN
    } else if m < 0.55 {
        super::theme::AMBER
    } else {
        GOOD
    }
}

fn biz_cell(r: &BizRow, c: BizCol) -> (String, Color32) {
    match c {
        BizCol::Name => (r.name.clone(), IVORY),
        BizCol::Kind => (r.kind_label.clone(), Color32::from_gray(200)),
        BizCol::Owner => (if r.owner.is_empty() { "independent".into() } else { format!("{} ({})", r.owner, r.owner_kind) }, if r.owner.is_empty() { QUIET } else { Color32::from_gray(210) }),
        BizCol::District => (r.district.clone(), Color32::from_rgb(200, 140, 230)),
        BizCol::Street => (r.street.clone(), Color32::from_gray(200)),
        BizCol::Staff => (format!("{}/{}", r.staff, r.jobs), if r.jobs > 0 && r.staff < r.jobs / 2 { WARN } else { IVORY }),
        BizCol::Today => (super::fmt::compact(r.revenue_today), IVORY),
        BizCol::Yesterday => (r.revenue_yesterday.map_or("-".into(), super::fmt::compact), IVORY),
        BizCol::Profit => r.profit_yesterday.map_or(("-".into(), QUIET), signed),
        BizCol::Money => (super::fmt::compact(r.money), if r.money < 0.0 { WARN } else { IVORY }),
        BizCol::Rating => (r.rating.map_or("-".into(), |s| format!("{s:.1}★")), r.rating.map_or(QUIET, |s| if s >= 3.5 { GOOD } else if s < 2.5 { WARN } else { IVORY })),
        BizCol::Footfall => (format!("{}", r.footfall), IVORY),
        BizCol::State => (r.state.label().into(), state_color(r.state)),
    }
}

/// [companies] A fleet colour as egui's, lightened when too dark to read.
pub fn livery_color(i: u8) -> Color32 {
    let [r, g, b] = nd_sim::companies::FLEET_COLOURS[i as usize % nd_sim::companies::FLEET_COLOURS.len()];
    if (r as u32 + g as u32 + b as u32) < 200 {
        Color32::from_rgb(r.saturating_add(90), g.saturating_add(90), b.saturating_add(100))
    } else {
        Color32::from_rgb(r, g, b)
    }
}

fn company_cell(r: &CompanyRow, c: CompanyCol) -> (String, Color32) {
    let money = |v: f64| super::fmt::money(v);
    match c {
        CompanyCol::Name => (r.name.clone(), r.livery.map_or(IVORY, livery_color)),
        CompanyCol::What => (r.what.clone(), if r.kind_key == "developer" { WARN } else { Color32::from_gray(200) }),
        CompanyCol::Founder => (r.founder.clone(), Color32::from_gray(200)),
        CompanyCol::Properties => (format!("{}", r.properties), IVORY),
        CompanyCol::Value => (money(r.value), IVORY),
        CompanyCol::Rent => (super::fmt::compact(r.rent), if r.rent > 0.0 { GOOD } else { QUIET }),
        CompanyCol::Takings => (super::fmt::compact(r.takings), if r.takings > 0.0 { GOOD } else { QUIET }),
        CompanyCol::Staff => (format!("{}", r.staff), IVORY),
        CompanyCol::Vehicles => (if r.vehicles > 0 { format!("{}", r.vehicles) } else { "-".into() }, if r.vehicles > 0 { IVORY } else { QUIET }),
        CompanyCol::Since => (format!("day {}", r.since_day + 1), Color32::from_gray(200)),
        CompanyCol::History => (r.latest.clone(), Color32::from_gray(190)),
    }
}

fn person_cell(r: &PersonRow, c: PersonCol) -> (String, Color32) {
    match c {
        PersonCol::Name => (r.name.clone(), IVORY),
        PersonCol::Age => (format!("{}", r.age), IVORY),
        PersonCol::Class => (crate::directory::class_key(r.class).into(), class_color(r.class)),
        PersonCol::Income => (format!("{}/d", super::fmt::compact(r.income)), IVORY),
        PersonCol::Job => (r.work_label.clone(), if r.work.is_some() { Color32::from_gray(210) } else if r.stage == nd_sim::citizens::LifeStage::Adult { WARN } else { QUIET }),
        PersonCol::Home => (if r.street.is_empty() { r.home_label.clone() } else { format!("{}, {}", r.home_label, r.street) }, if r.home.is_some() { Color32::from_gray(210) } else { WARN }),
        PersonCol::Household => (format!("{}", r.household), IVORY),
        PersonCol::Mood => (format!("{:.0}%", r.mood * 100.0), mood_color(r.mood)),
        PersonCol::Health => {
            if r.ill {
                ("ill".into(), WARN)
            } else if r.hungry_days > 0 {
                (format!("hungry {}d", r.hungry_days), super::theme::AMBER)
            } else {
                (format!("fed {:.0}%", r.satiety * 100.0), if r.satiety < 0.3 { super::theme::AMBER } else { IVORY })
            }
        }
        PersonCol::Commute => (r.commute.map_or("-".into(), |m| m.key().into()), Color32::from_gray(200)),
        PersonCol::Favourite => (r.favourite.clone(), Color32::from_gray(200)),
        PersonCol::Friends => (format!("{}", r.friends), IVORY),
        PersonCol::Status => (r.status.label().into(), status_color(r.status)),
    }
}
