//! The top bar: title, clock and speed, treasury and population, the
//! toggles that open the info windows (Dev among them), and the camera mode.
//! Two lines, read left to right like a city builder's status strip: the
//! city's figures (the map layer at the right), then the window toggles
//! (view and camera at the right).
//!
//! Nothing may overlap at any window width or text size, so each line is
//! laid out in groups whose widths are known before drawing (measured the
//! frame before, or from the text): a group that does not fit moves to a
//! further row, the status message truncates, and the window toggles that
//! do not fit fold into a "More" menu.

use bevy_egui::egui;

use super::windows::Info;
use super::{help, neon, section, UiCtx, GOLD, IVORY, WARN};
use crate::map_layers::{self, LayerId};
use crate::render::Overlays;
use crate::camera::CameraMode;
use crate::sim::SPEEDS;

/// Space between items in the bar.
const ITEM_GAP: f32 = 6.0;
/// What a separator between two groups takes: its own width plus the item
/// gap on either side.
const GROUP_GAP: f32 = 6.0 + 2.0 * ITEM_GAP;
/// The narrowest the status message gets before it moves to its own row.
const STATUS_MIN: f32 = 110.0;
/// How many window toggles stay out of the "More" menu however narrow.
const TOGGLES_MIN: usize = 2;
const MORE: &str = "More \u{25BE}";

/// The bar's groups, in reading order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
enum Group {
    /// Title, clock, pause and speeds.
    Clock,
    /// Treasury, population, power.
    Money,
    /// "Connect a road to the highway..." while nobody can move in.
    Status,
    /// The map layer picker.
    Layer,
    /// The info window toggles.
    Windows,
    /// The view depth, heat vision and the camera mode.
    Camera,
}

/// A group to place: the width it wants, the least it can make do with
/// (equal for fixed groups), and whether it hugs the right edge.
#[derive(Clone, Copy, Debug)]
struct Item {
    group: Group,
    want: f32,
    min: f32,
    right: bool,
}

/// Lay a line's items out in rows of `width`: greedily, an item that does
/// not fit at its least width starts a new row; then each row's spare width
/// goes to its flexible items, in proportion to what they still want.
/// Returns each row's items with the width they get.
fn flow(items: &[Item], width: f32) -> Vec<Vec<(Item, f32)>> {
    let mut rows: Vec<Vec<Item>> = Vec::new();
    let mut used = 0.0;
    for it in items {
        match rows.last_mut() {
            Some(row) if used + GROUP_GAP + it.min <= width => {
                row.push(*it);
                used += GROUP_GAP + it.min;
            }
            _ => {
                rows.push(vec![*it]);
                used = it.min;
            }
        }
    }
    rows.into_iter()
        .map(|row| {
            let gaps = GROUP_GAP * (row.len() as f32 - 1.0);
            let least: f32 = row.iter().map(|i| i.min).sum::<f32>() + gaps;
            let spare = (width - least).max(0.0);
            let wanting: f32 = row.iter().map(|i| i.want - i.min).sum();
            let share = if wanting > 0.0 { (spare / wanting).min(1.0) } else { 0.0 };
            row.into_iter().map(|i| (i, i.min + (i.want - i.min) * share)).collect()
        })
        .collect()
}

pub fn show(ui: &mut egui::Ui, p: &mut UiCtx) {
    ui.spacing_mut().item_spacing.x = ITEM_GAP;
    // A hair of slack so rounding never pushes the last item past the edge.
    let width = ui.available_width() - 2.0;
    let fixed = |ui: &egui::Ui, g: Group, guess: f32| {
        let w = ui.data(|d| d.get_temp::<f32>(width_id(g))).unwrap_or(guess);
        Item { group: g, want: w, min: w, right: false }
    };
    // The layer picker before the status message: the message is the one
    // that truncates, so it takes what the picker leaves (and, when even
    // its least does not fit, both go on a row of their own).
    let mut first = vec![fixed(ui, Group::Clock, 560.0), fixed(ui, Group::Money, 230.0), Item { right: true, ..fixed(ui, Group::Layer, 190.0) }];
    if let Some(text) = status_text(p) {
        let want = text_width(ui, status_rich(text), egui::TextStyle::Small);
        first.push(Item { group: Group::Status, want, min: want.min(STATUS_MIN), right: false });
    }
    let toggles = toggle_widths(ui);
    let all: f32 = toggles.iter().map(|t| t.1).sum();
    let least = toggles.iter().take(TOGGLES_MIN).map(|t| t.1).sum::<f32>() + more_width(ui);
    let second = [Item { group: Group::Windows, want: all, min: least.min(all), right: false }, Item { right: true, ..fixed(ui, Group::Camera, 420.0) }];
    for row in flow(&first, width).into_iter().chain(flow(&second, width)) {
        ui.horizontal(|ui| draw_row(ui, p, &row));
    }
}

fn width_id(g: Group) -> egui::Id {
    egui::Id::new(("top_bar_width", g))
}

/// One row: its left groups with separators between, then its right groups
/// pushed to the edge.
fn draw_row(ui: &mut egui::Ui, p: &mut UiCtx, row: &[(Item, f32)]) {
    for (i, (it, w)) in row.iter().filter(|(it, _)| !it.right).enumerate() {
        if i > 0 {
            ui.separator();
        }
        draw(ui, p, it.group, *w);
    }
    let right: Vec<_> = row.iter().filter(|(it, _)| it.right).collect();
    if right.is_empty() {
        return;
    }
    let need = right.iter().map(|(_, w)| *w).sum::<f32>() + GROUP_GAP * (right.len() as f32 - 1.0);
    let spare = ui.available_width() - need;
    if spare > 0.0 {
        ui.add_space(spare);
    }
    for (i, (it, w)) in right.iter().enumerate() {
        if i > 0 {
            ui.separator();
        }
        draw(ui, p, it.group, *w);
    }
}

/// Draw a group in `width`. Fixed groups record the width they took, for
/// the layout (and redo the frame when it changed, so a figure growing a
/// digit never overlaps anything for even a frame).
fn draw(ui: &mut egui::Ui, p: &mut UiCtx, g: Group, width: f32) {
    let taken = match g {
        Group::Status => {
            let text = status_text(p).unwrap_or_default();
            ui.allocate_ui_with_layout(egui::vec2(width, ui.spacing().interact_size.y), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.set_max_width(width);
                ui.add(egui::Label::new(status_rich(text)).truncate()).on_hover_text(format!("{text}. Newcomers, their cars and lorries from out of town arrive by the highway (join a street to it with ramps), by a road drawn off the edge of the map, or by train at a working station. Until then nobody can come."));
            });
            return;
        }
        Group::Windows => {
            window_toggles(ui, width);
            return;
        }
        Group::Clock => ui.horizontal(|ui| {
            ui.label(egui::RichText::new("DISTRICT 2040").color(GOLD).strong().font(crate::ui::theme::lead()));
            ui.separator();
            clock_and_speed(ui, p);
        }),
        Group::Money => ui.horizontal(|ui| treasury_and_population(ui, p)),
        Group::Layer => ui.horizontal(|ui| layer_button(ui, p)),
        Group::Camera => ui.horizontal(|ui| {
            // The layer the view shows: Underground, Ground, Sky [U].
            super::tools::depth_switch(ui, p.tools);
            ui.separator();
            camera(ui, p);
        }),
    }
    .response
    .rect
    .width();
    let id = width_id(g);
    let before = ui.data(|d| d.get_temp::<f32>(id));
    if before.is_none_or(|b| (b - taken).abs() > 0.5) {
        ui.data_mut(|d| d.insert_temp(id, taken));
        ui.ctx().request_discard("top bar group width changed");
    }
}

/// The status message, when there is one: nobody can move in until the
/// city's streets reach elsewhere (the highway by a ramp, a road off the
/// map edge, or a working station).
fn status_text(p: &UiCtx) -> Option<&'static str> {
    (!p.sim.world.has_way_in()).then_some("Connect a road to the highway so people can move in")
}

fn status_rich(text: &str) -> egui::RichText {
    egui::RichText::new(text).small().color(WARN)
}

/// The width a text takes on one line in `style`.
fn text_width(ui: &egui::Ui, text: impl Into<egui::WidgetText>, style: egui::TextStyle) -> f32 {
    text.into().into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, style).size().x
}

/// The width a selectable label with `text` takes, with the gap after it.
fn label_width(ui: &egui::Ui, text: &str) -> f32 {
    text_width(ui, text, egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.x + ITEM_GAP
}

fn more_width(ui: &egui::Ui) -> f32 {
    label_width(ui, MORE)
}

/// The window toggles in bar order: `New game…` before Saves, a separator
/// before it and before Dev.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Toggle {
    Window(Info),
    NewGame,
}

impl Toggle {
    fn all() -> Vec<Toggle> {
        let mut out = Vec::new();
        for info in Info::ALL {
            if info == Info::Saves {
                out.push(Toggle::NewGame);
            }
            out.push(Toggle::Window(info));
        }
        out
    }

    fn separated(self) -> bool {
        matches!(self, Toggle::NewGame | Toggle::Window(Info::Dev))
    }

    fn label(self) -> &'static str {
        match self {
            Toggle::Window(info) => info.title(),
            Toggle::NewGame => "New game…",
        }
    }

    fn is_open(self, ctx: &egui::Context) -> bool {
        match self {
            Toggle::Window(info) => info.is_open(ctx),
            Toggle::NewGame => super::newgame::is_open(ctx),
        }
    }

    fn show(self, ui: &mut egui::Ui) {
        match self {
            Toggle::NewGame => super::newgame::toggle(ui),
            Toggle::Window(info) => {
                let open = info.is_open(ui.ctx());
                if ui.selectable_label(open, info.title()).clicked() {
                    info.set_open(ui.ctx(), !open);
                }
            }
        }
    }
}

/// Each toggle with the width it takes in the bar, its separator included.
fn toggle_widths(ui: &egui::Ui) -> Vec<(Toggle, f32)> {
    Toggle::all().into_iter().map(|t| (t, label_width(ui, t.label()) + if t.separated() { GROUP_GAP - ITEM_GAP } else { 0.0 })).collect()
}

/// One toggle per info window, lit while the window is open; the ones that
/// do not fit in `width` go in a "More" menu at the end, lit while one of
/// them is open.
fn window_toggles(ui: &mut egui::Ui, width: f32) {
    let toggles = toggle_widths(ui);
    let all: f32 = toggles.iter().map(|t| t.1).sum();
    let shown = if all <= width + 0.5 {
        toggles.len()
    } else {
        let room = width - more_width(ui);
        let mut used = 0.0;
        toggles
            .iter()
            .take_while(|t| {
                used += t.1;
                used <= room
            })
            .count()
            .max(TOGGLES_MIN)
    };
    ui.horizontal(|ui| {
        for (t, _) in &toggles[..shown] {
            if t.separated() {
                ui.separator();
            }
            t.show(ui);
        }
        let rest: Vec<Toggle> = toggles[shown..].iter().map(|t| t.0).collect();
        if rest.is_empty() {
            return;
        }
        let lit = rest.iter().any(|t| t.is_open(ui.ctx()));
        // Looks like the toggles it stands for, lit like them.
        let (resp, _) = egui::containers::menu::MenuButton::from_button(egui::Button::selectable(lit, MORE)).ui(ui, |ui| {
            for t in rest {
                if t.separated() {
                    ui.separator();
                }
                t.show(ui);
            }
        });
        resp.on_hover_text("The windows the bar has no room for");
    });
}

fn clock_and_speed(ui: &mut egui::Ui, p: &mut UiCtx) {
    let sim = &mut *p.sim;
    neon(ui, &format!("Day {}  {}", sim.world.clock.day() + 1, sim.world.clock.format_hhmm()));
    let label = if sim.paused { "▶" } else { "⏸" };
    if ui.button(label).on_hover_text(if sim.paused { "Run [Space]" } else { "Pause [Space]" }).clicked() {
        sim.paused = !sim.paused;
    }
    for (i, (label, _)) in SPEEDS.iter().enumerate() {
        if ui.selectable_label(sim.speed_index == i, *label).on_hover_text("- / = change speed").clicked() {
            sim.speed_index = i;
        }
    }
    // Effective speed: the share of the chosen speed the sim is actually
    // delivering. Below 0.9 the tick budget is throttling it, and that is
    // worth a red figure in the bar; at full speed it says nothing.
    let eff = sim.effective_speed;
    let text = if sim.paused {
        egui::RichText::new("paused").small().weak()
    } else if eff < 0.9 {
        egui::RichText::new(format!("{:.0}%", eff * 100.0)).small().color(WARN)
    } else {
        egui::RichText::new("")
    };
    slot(ui, 40.0, text).on_hover_text(format!("The sim is running at {:.0}% of {} (below 90% the tick budget is throttling it).", eff * 100.0, SPEEDS[sim.speed_index].0));
}

/// A label in a fixed-width slot, so figures that come, go or change length
/// never shift the window toggles to their right.
fn slot(ui: &mut egui::Ui, width: f32, text: egui::RichText) -> egui::Response {
    ui.add_sized(egui::vec2(width, ui.spacing().interact_size.y), egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend))
}

fn treasury_and_population(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let e = &w.economy;
    let treasury = egui::RichText::new(format!("$ {:.0}", e.treasury)).strong();
    let treasury = if e.treasury < 0.0 { treasury.color(WARN) } else { treasury.color(GOLD) };
    let resp = ui
        .add_sized(egui::vec2(80.0, ui.spacing().interact_size.y), egui::Label::new(treasury).wrap_mode(egui::TextWrapMode::Extend).sense(egui::Sense::click()))
        .on_hover_text(format!("treasury · taxes so far today {:.0}", e.tax_today));
    dev_grant(ui, &resp, p);
    let w = &p.sim.world;
    let e = &w.economy;
    slot(ui, 64.0, egui::RichText::new(format!("pop {}", w.citizens.len())).strong()).on_hover_text(format!("{} homes · {} jobs · {} waiting to move in", e.homes_total, e.jobs_total, e.waiting_immigrants));
    let power = &e.power;
    // No demand yet (an empty map) reads as 0% served: nothing to warn about.
    let text = if power.demand_mw > 0.0 && power.served < 0.98 { egui::RichText::new(format!("⚡ {:.0}%", power.served * 100.0)).small().color(WARN) } else { egui::RichText::new("") };
    slot(ui, 40.0, text).on_hover_text(format!("power served {:.0}%: the Power window has the detail", power.served * 100.0));
}

/// Dev: how long the treasury figure must be held down for a grant.
const GRANT_HOLD_SECS: f64 = 10.0;
/// Dev: what each full hold adds to the treasury.
const GRANT_AMOUNT: f32 = 1_000_000.0;

/// Dev cheat: hold the mouse down on the treasury figure for ten seconds and
/// a million goes into the treasury; keep holding for another million every
/// ten seconds. A tooltip counts the hold down so it is clear it is working.
fn dev_grant(ui: &mut egui::Ui, resp: &egui::Response, p: &mut UiCtx) {
    let id = resp.id.with("dev_grant_since");
    let now = ui.input(|i| i.time);
    if !resp.is_pointer_button_down_on() {
        ui.data_mut(|d| d.remove::<f64>(id));
        return;
    }
    let since = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || now));
    let held = now - since;
    if held >= GRANT_HOLD_SECS {
        p.sim.world.economy.treasury += GRANT_AMOUNT;
        p.tools.status = format!("Dev: +$ {GRANT_AMOUNT:.0} to the treasury");
        ui.data_mut(|d| d.insert_temp(id, now));
    } else if held > 0.5 {
        resp.show_tooltip_text(format!("dev grant: +$1,000,000 in {:.0} s", (GRANT_HOLD_SECS - held).ceil()));
    }
    ui.ctx().request_repaint();
}

fn camera(ui: &mut egui::Ui, p: &mut UiCtx) {
    let heat = p.lighting.heat;
    if ui.selectable_label(heat, "Heat").on_hover_text("Drone heat vision [H]: buildings go dark, people and engines glow").clicked() {
        p.lighting.heat = !heat;
    }
    ui.separator();
    let walking = *p.mode == CameraMode::Walk;
    if ui.selectable_label(walking, "Walk").on_hover_text(format!("Walk the street [Tab]. {}", help::CAMERA_WALK)).clicked() {
        *p.mode = CameraMode::Walk;
    }
    if ui.selectable_label(!walking, "Orbit").on_hover_text(help::CAMERA_ORBIT).clicked() {
        *p.mode = CameraMode::Orbit;
    }
}

/// The map layer button: the layer showing (or None), a dropdown to pick
/// another. Layers are exclusive; the legend shows the key of the one on.
fn layer_button(ui: &mut egui::Ui, p: &mut UiCtx) {
    let active = map_layers::active(p.overlays);
    let text = match active {
        Some(l) => egui::RichText::new(format!("Layer: {}", l.label())).color(GOLD).strong(),
        None => egui::RichText::new("Layer: none").color(IVORY),
    };
    egui::ComboBox::from_id_salt("map_layer")
        .selected_text(text)
        .width(170.0)
        .height(620.0)
        .show_ui(ui, |ui| layer_list(ui, p.overlays, false))
        .response
        .on_hover_text("The map layer: one data view at a time, with its colour key at the bottom right. V, O, P toggle theirs; L steps through the heat maps, K through the land layers.");
}

/// Every map layer under its group heading, the one on lit; a click shows
/// it alone, "None" clears the map. `wrap` lays each group out in a
/// wrapping row (the View window) rather than a column (the dropdown).
pub fn layer_list(ui: &mut egui::Ui, overlays: &mut Overlays, wrap: bool) {
    let active = map_layers::active(overlays);
    let mut pick: Option<Option<LayerId>> = None;
    if ui.selectable_label(active.is_none(), "None").on_hover_text("No data layer: the plain map").clicked() {
        pick = Some(None);
    }
    let all = LayerId::all();
    let mut i = 0;
    while i < all.len() {
        let group = all[i].group();
        let members: Vec<LayerId> = all[i..].iter().copied().take_while(|l| l.group() == group).collect();
        i += members.len();
        section(ui, &group.to_uppercase());
        let mut item = |ui: &mut egui::Ui, l: LayerId| {
            let key = l.hotkey();
            let label = if key.is_empty() || wrap { l.label().to_string() } else { format!("{}  [{key}]", l.label()) };
            if ui.selectable_label(active == Some(l), label).on_hover_text(l.hint()).clicked() {
                pick = Some(if active == Some(l) { None } else { Some(l) });
            }
        };
        if wrap {
            ui.horizontal_wrapped(|ui| members.iter().for_each(|l| item(ui, *l)));
        } else {
            members.iter().for_each(|l| item(ui, *l));
        }
    }
    if let Some(l) = pick {
        map_layers::set_active(overlays, l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(group: Group, want: f32, min: f32, right: bool) -> Item {
        Item { group, want, min, right }
    }

    fn row_width(row: &[(Item, f32)]) -> f32 {
        row.iter().map(|r| r.1).sum::<f32>() + GROUP_GAP * (row.len() as f32 - 1.0)
    }

    /// No row is ever wider than the bar, at any width, as long as every
    /// item alone fits; flexible items never get less than their least.
    #[test]
    fn rows_never_overflow() {
        let items = [
            item(Group::Clock, 520.0, 520.0, false),
            item(Group::Money, 210.0, 210.0, false),
            item(Group::Layer, 190.0, 190.0, true),
            item(Group::Status, 330.0, 110.0, false),
        ];
        let mut w = 520.0;
        while w < 2600.0 {
            let rows = flow(&items, w);
            assert_eq!(rows.iter().map(Vec::len).sum::<usize>(), items.len());
            for row in &rows {
                assert!(row_width(row) <= w + 0.01, "row {row:?} wider than {w}");
                for (it, got) in row {
                    assert!(*got >= it.min - 0.01 && *got <= it.want + 0.01);
                }
            }
            w += 7.0;
        }
    }

    /// Wide enough, a line is one row and the flexible item gets all it wants;
    /// a little narrower, it truncates rather than wrapping.
    #[test]
    fn flex_shrinks_before_wrapping() {
        let items = [item(Group::Windows, 800.0, 200.0, false), item(Group::Camera, 400.0, 400.0, true)];
        let rows = flow(&items, 1300.0);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0].1, 800.0);
        let rows = flow(&items, 900.0);
        assert_eq!(rows.len(), 1);
        assert!((row_width(&rows[0]) - 900.0).abs() < 0.01);
        assert_eq!(flow(&items, 500.0).len(), 2);
    }
}
