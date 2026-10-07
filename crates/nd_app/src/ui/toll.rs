//! [toll-inspect] The inspector on a selected toll booth: what it is and
//! where, its toll (with a quick adjuster), the vehicles through and the
//! money taken today, yesterday and all along, lorries and free passes,
//! the drivers who went round it, the queue now, upkeep and profit, and
//! its traffic hour by hour over the last three days.

use bevy_egui::egui::{self, Color32, Stroke};

use super::{hint, section, UiCtx, GOLD, IVORY, WARN};
use crate::sim::Sim;
use crate::tools::Selection;
use nd_sim::tolls::{TollHour, MAX_PRICE, TRUCK_FACTOR};

const LORRY: Color32 = Color32::from_rgb(240, 140, 60);
const FREE: Color32 = Color32::from_rgb(110, 200, 220);
const ROUND: Color32 = Color32::from_rgb(255, 90, 110);
const PLOT_BG: Color32 = Color32::from_rgba_premultiplied(20, 22, 34, 220);

/// The inspector window on a selected booth: the same window (id, place
/// and size) the inspector opens on anything else. Closing it deselects.
pub fn window(ctx: &egui::Context, p: &mut UiCtx, content: egui::Rect) {
    let Selection::Toll(id) = *p.selection else { return };
    let mut open = true;
    egui::Window::new(egui::RichText::new("Inspector").color(GOLD).strong())
        .id(egui::Id::new("inspector_window"))
        .open(&mut open)
        .default_pos(egui::pos2(content.right() - 392.0, content.top() + 12.0))
        .default_width(360.0)
        // Tall enough for the chart without scrolling.
        .default_height((content.height() - 24.0).min(780.0))
        .constrain_to(content)
        .max_height(content.height() - 24.0)
        .vscroll(true)
        .show(ctx, |ui| {
            show(ui, p.sim, p.selection, id);
            ui.separator();
            super::follow::camera_buttons(ui, p.sim, *p.selection, p.follow, p.fly, p.mode);
            if ui.button("Flag for Claude (F8)").on_hover_text("Bookmark this with a note; the remote API's `flags` command lists it").clicked() {
                p.flags.want_flag = true;
            }
        });
    if !open {
        *p.selection = Selection::None;
    }
}

pub fn show(ui: &mut egui::Ui, sim: &mut Sim, selection: &mut Selection, id: u32) {
    let w = &sim.world;
    let Some(b) = w.tolls.get(id) else {
        *selection = Selection::None;
        return;
    };
    ui.label(egui::RichText::new(&b.name).font(crate::ui::theme::lead()).color(IVORY).strong());
    let (road, ways) = crate::toll_select::road(w, b);
    let lanes: u8 = ways.iter().map(|(_, n)| n).sum();
    let kiosks = b.size.booths(lanes);
    hint(ui, &format!("{}  ·  {kiosks} kiosk{} across  ·  on a {road}", capitalise(b.size.label()), if kiosks == 1 { "" } else { "s" }));
    let charges = ways.iter().map(|(d, n)| format!("{n} lane{} {d}bound", if *n == 1 { "" } else { "s" })).collect::<Vec<_>>().join(", ");
    ui.label(format!("Charges {}: {charges}", if ways.len() > 1 { "both ways" } else { "one way" }));

    // The toll, with a quick adjuster and the way to the Taxes window.
    section(ui, "TOLL");
    let mut price = b.price;
    let mut set = None;
    ui.horizontal(|ui| {
        if ui.small_button("-").on_hover_text("0.5 less").clicked() {
            set = Some((price - 0.5).max(0.0));
        }
        ui.spacing_mut().slider_width = 130.0;
        if ui.add(egui::Slider::new(&mut price, 0.0..=MAX_PRICE.min(20.0)).step_by(0.5).suffix(" a car")).changed() {
            set = Some(price);
        }
        if ui.small_button("+").on_hover_text("0.5 more").clicked() {
            set = Some((price + 0.5).min(MAX_PRICE));
        }
    });
    hint(ui, &format!("Lorries pay {:.1}; buses, emergency and city vehicles go free. Drivers count a toll as {:.0} m of detour per unit of money, so a dear one sends some round.", b.price * TRUCK_FACTOR, nd_sim::tolls::TOLL_AVERSION_M));
    if ui.small_button("Every booth in the Taxes window").clicked() {
        super::Info::Taxes.set_open(ui.ctx(), true);
    }

    // Through, today / yesterday / all along.
    section(ui, "TRAFFIC");
    egui::Grid::new(("toll_books", id)).num_columns(4).spacing(egui::vec2(14.0, 2.0)).show(ui, |ui| {
        ui.label("");
        for h in ["today", "yesterday", "all time"] {
            ui.label(egui::RichText::new(h).small().weak());
        }
        ui.end_row();
        let mut row = |name: &str, col: Color32, tip: &str, v: [String; 3]| {
            ui.label(egui::RichText::new(name).color(col)).on_hover_text(tip);
            for x in v {
                ui.label(egui::RichText::new(x).strong());
            }
            ui.end_row();
        };
        row("Vehicles through", IVORY, "Every vehicle that drove through the booth line, lorries and free ones included.", [b.cars_today.to_string(), b.cars_yesterday.to_string(), b.cars_total.to_string()]);
        let cars = |t: u32, l: u32, f: u32| t.saturating_sub(l + f).to_string();
        row("  cars paying", GOLD, "Cars that paid the toll.", [cars(b.cars_today, b.trucks_today, b.free_today), cars(b.cars_yesterday, b.trucks_yesterday, b.free_yesterday), b.cars_total.saturating_sub(b.trucks_total + b.free_total).to_string()]);
        row("  lorries", LORRY, "Delivery lorries, paying double out of their business's money.", [b.trucks_today.to_string(), b.trucks_yesterday.to_string(), b.trucks_total.to_string()]);
        row("  free", FREE, "Buses, emergency vehicles and city vehicles: let through without paying.", [b.free_today.to_string(), b.free_yesterday.to_string(), b.free_total.to_string()]);
        row("Went round it", ROUND, "Drivers who set off another way that they would not have taken were there no tolls: the toll sent them round.", [b.avoided_today.to_string(), b.avoided_yesterday.to_string(), b.avoided_total.to_string()]);
        row("Revenue", IVORY, "Tolls taken, into the treasury under road tolls.", [format!("{:.0}", b.revenue_today), format!("{:.0}", b.revenue_yesterday), format!("{:.0}", b.revenue_total)]);
    });
    let upkeep = b.size.upkeep_per_day();
    let net = b.net_per_day();
    let full_day = b.cars_yesterday > 0 || b.revenue_yesterday > 0.0;
    ui.horizontal(|ui| {
        ui.label(format!("Upkeep {upkeep:.0}/day  ·  net"));
        ui.label(egui::RichText::new(format!("{net:+.0}/day")).color(if net >= 0.0 { Color32::from_rgb(120, 230, 150) } else { WARN }).strong())
            .on_hover_text(if full_day { "Yesterday's takings less the staff and upkeep paid at midnight." } else { "Today's takings so far less the staff and upkeep paid at midnight (no full day yet)." });
    });
    if b.avoided_today > 0 {
        let share = (b.cars_today + b.avoided_today).max(1) as f32;
        hint(ui, &format!("{:.0}% of the drivers who would have come through today went round", b.avoided_today as f32 * 100.0 / share));
    }

    // The queue now.
    section(ui, "QUEUE NOW");
    match w.toll_queue(id) {
        Some((n, back, mins)) if n > 0 => {
            let col = if mins >= 5.0 { WARN } else { IVORY };
            ui.label(egui::RichText::new(format!("{n} vehicle{} waiting, back {back:.0} m  ·  about {} for the last", if n == 1 { "" } else { "s" }, minutes(mins))).color(col));
        }
        _ => hint(ui, "nobody waiting: traffic rolls through"),
    }

    section(ui, "HOUR BY HOUR");
    let hours: Vec<TollHour> = b.hourly.iter().copied().collect();
    hour_chart(ui, &hours, &b.hour, w.clock.day() as u32 + 1, w.clock.hour() as u8);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        key(ui, GOLD, "cars");
        key(ui, LORRY, "lorries");
        key(ui, FREE, "free");
        key(ui, ROUND, "went round");
    });

    ui.add_space(4.0);
    let remove = ui.button(egui::RichText::new("Remove booth").color(WARN)).on_hover_text("Take it away (no refund: the money went into the concrete).").clicked();
    if let Some(p) = set {
        let _ = sim.world.set_toll_price(id, p);
    }
    if remove {
        let _ = sim.world.remove_toll(id);
        *selection = Selection::None;
    }
}

fn minutes(m: f32) -> String {
    if m < 1.0 {
        "under a minute".into()
    } else {
        format!("{m:.0} min")
    }
}

fn key(ui: &mut egui::Ui, col: Color32, text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 2.0, col);
        ui.label(egui::RichText::new(text).small().weak());
    });
}

/// Bars of vehicles through each hour (cars, lorries and free ones
/// stacked) over the kept hours plus the one under way (dimmed), with the
/// drivers who went round as a red line, midnights marked, and the hour
/// under the pointer read out.
fn hour_chart(ui: &mut egui::Ui, hours: &[TollHour], now: &TollHour, day_now: u32, hour_now: u8) {
    let (resp, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), 96.0), egui::Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 2.0, PLOT_BG);
    let small = crate::ui::theme::small();
    let tick = Color32::from_gray(170);
    if hours.is_empty() && now.cars == 0 && now.avoided == 0 {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "the chart fills in hour by hour", small, tick);
        return;
    }
    let mut all: Vec<TollHour> = hours.to_vec();
    all.push(TollHour { day: day_now, hour: hour_now, ..*now });
    let n = all.len();
    let max = all.iter().map(|h| h.cars.max(h.avoided)).max().unwrap_or(1).max(1) as f32;
    let plot = egui::Rect::from_min_max(rect.min + egui::vec2(4.0, 13.0), rect.max - egui::vec2(4.0, 15.0));
    let bw = plot.width() / n.max(24) as f32;
    let y = |v: f32| plot.bottom() - plot.height() * v / max;
    for (i, h) in all.iter().enumerate() {
        let x0 = plot.left() + i as f32 * bw;
        let fade = if i + 1 == n { 0.45 } else { 1.0 };
        let paying = h.cars.saturating_sub(h.trucks + h.free) as f32;
        let mut base = 0.0;
        for (v, col) in [(paying, GOLD), (h.trucks as f32, LORRY), (h.free as f32, FREE)] {
            if v > 0.0 {
                let r = egui::Rect::from_min_max(egui::pos2(x0 + 0.5, y(base + v)), egui::pos2(x0 + bw - 0.5, y(base)));
                painter.rect_filled(r, 0.0, col.gamma_multiply(fade));
            }
            base += v;
        }
        if h.hour == 0 {
            painter.line_segment([egui::pos2(x0, plot.top()), egui::pos2(x0, rect.bottom())], Stroke::new(1.0, Color32::from_gray(90)));
            painter.text(egui::pos2(x0 + 2.0, rect.bottom() - 1.0), egui::Align2::LEFT_BOTTOM, format!("day {}", h.day), small.clone(), tick);
        }
    }
    if all.iter().any(|h| h.avoided > 0) && n > 1 {
        let pts: Vec<egui::Pos2> = all.iter().enumerate().map(|(i, h)| egui::pos2(plot.left() + (i as f32 + 0.5) * bw, y(h.avoided as f32))).collect();
        painter.line(pts, Stroke::new(1.5, ROUND));
    }
    painter.text(rect.left_top() + egui::vec2(4.0, 1.0), egui::Align2::LEFT_TOP, format!("{max:.0} an hour"), small.clone(), tick);
    painter.text(rect.right_top() + egui::vec2(-4.0, 1.0), egui::Align2::RIGHT_TOP, format!("last {} h", n - 1), small, tick);
    if let Some(pos) = resp.hover_pos() {
        let i = (((pos.x - plot.left()) / bw).floor().max(0.0) as usize).min(n - 1);
        let x0 = plot.left() + i as f32 * bw;
        painter.rect_stroke(egui::Rect::from_min_max(egui::pos2(x0, plot.top()), egui::pos2(x0 + bw, plot.bottom())), 0.0, Stroke::new(1.0, Color32::from_gray(150)), egui::StrokeKind::Inside);
        let h = all[i];
        let when = if i + 1 == n { format!("Day {}, {:02}:00, so far", h.day, h.hour) } else { format!("Day {}, {:02}:00", h.day, h.hour) };
        resp.on_hover_text(format!("{when}\n{} through ({} lorries, {} free)\n{} went round\n{:.0} taken", h.cars, h.trucks, h.free, h.avoided, h.revenue));
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}
