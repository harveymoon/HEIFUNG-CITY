//! [road-inspect] The card on a selected street piece: what it is (street
//! name, kind, lanes each way, limit, length, level, one-way), its traffic
//! (on it now by kind, speed against the limit, congestion and the queue,
//! vehicles an hour and the last three days hour by hour), its decay (wear
//! and condition, what the rough surface does to drivers, wear a day and
//! days to the next condition, repair cost, its place in the crews' queue,
//! the last repair, Repair now) and what stands on it (toll booths,
//! crosswalks, bus stops, kerb parking).
//!
//! The figures are per piece: a street between two junctions is often
//! several pieces (every bend is one), and its traffic and wear differ
//! from piece to piece (a bus stop, a side lot's driveway, a bridge deck
//! wearing faster), so the card stays on the piece clicked and says where
//! it sits in its run, which is traced faintly on the map.
//!
//! In Inspect mode it is the inspector window; in Edit mode, with a road
//! picked for upgrading, a compact copy sits above the toolbar beside the
//! ROAD UPGRADE panel.

use bevy_egui::egui::{self, Color32, Stroke};

use super::{hint, section, UiCtx, GOLD, IVORY, WARN};
use crate::road_select::Card;
use crate::sim::Sim;
use crate::tools::{Selection, Tool};

const GOOD: Color32 = Color32::from_rgb(120, 230, 150);
const FAIR: Color32 = Color32::from_rgb(230, 210, 110);
const BAD: Color32 = Color32::from_rgb(255, 150, 70);
const PLOT_BG: Color32 = Color32::from_rgba_premultiplied(20, 22, 34, 220);

/// The road card's window: the inspector's (same id, place and size) in
/// Inspect mode; a compact one over the toolbar in Edit mode. Closing it
/// deselects.
pub fn window(ctx: &egui::Context, p: &mut UiCtx, content: egui::Rect) {
    let Selection::Road(sid) = *p.selection else { return };
    let mut open = true;
    let mut msg = None;
    if p.tools.tool == Tool::Edit {
        // The road picked for upgrading was let go: so is its card.
        if p.tools.selected_gesture.is_none() {
            *p.selection = Selection::None;
            return;
        }
        egui::Window::new(egui::RichText::new("Road").color(GOLD).strong())
            .id(egui::Id::new("road_card_edit"))
            .open(&mut open)
            .default_pos(egui::pos2(content.right() - 372.0, content.bottom() - 420.0))
            .default_width(340.0)
            .constrain_to(content)
            .max_height(content.height() - 24.0)
            .vscroll(true)
            .show(ctx, |ui| {
                msg = show(ui, p.sim, p.selection, sid, true);
            });
    } else {
        egui::Window::new(egui::RichText::new("Inspector").color(GOLD).strong())
            .id(egui::Id::new("inspector_window"))
            .open(&mut open)
            .default_pos(egui::pos2(content.right() - 392.0, content.top() + 12.0))
            .default_width(360.0)
            .default_height((content.height() - 24.0).min(820.0))
            .constrain_to(content)
            .max_height(content.height() - 24.0)
            .vscroll(true)
            .show(ctx, |ui| {
                msg = show(ui, p.sim, p.selection, sid, false);
                ui.separator();
                super::follow::camera_buttons(ui, p.sim, *p.selection, p.follow, p.fly, p.mode);
                if ui.button("Flag for Claude (F8)").on_hover_text("Bookmark this with a note; the remote API's `flags` command lists it").clicked() {
                    p.flags.want_flag = true;
                }
            });
    }
    if let Some(m) = msg {
        p.tools.status = m;
    }
    if !open {
        *p.selection = Selection::None;
    }
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The card. `compact` (Edit mode) leaves out the chart and the list of
/// what stands on it. Returns a message for the status line when Repair
/// now was pressed.
pub fn show(ui: &mut egui::Ui, sim: &mut Sim, selection: &mut Selection, sid: nd_sim::SegmentId, compact: bool) -> Option<String> {
    let Some(card) = Card::of(&sim.world, sid) else {
        *selection = Selection::None;
        return None;
    };
    let w = &sim.world;
    ui.label(egui::RichText::new(&card.name).font(crate::ui::theme::lead()).color(IVORY).strong());
    hint(ui, &identity_line(&card));
    if card.run.len() > 1 {
        hint(ui, &format!("Piece {} of {} between junctions ({:.0} m in all; the rest traced faintly). Figures are for this piece.", card.run_index + 1, card.run.len(), card.run_length));
    }

    // Traffic now.
    section(ui, "TRAFFIC NOW");
    let n = &card.now;
    let mut kinds = Vec::new();
    for (k, one, many) in [(n.cars, "car", "cars"), (n.lorries, "lorry", "lorries"), (n.buses, "bus", "buses"), (n.sirens, "emergency or city vehicle", "emergency or city vehicles")] {
        if k > 0 {
            kinds.push(plural(k, one, many));
        }
    }
    let vehicles = if kinds.is_empty() { "No vehicles on it".to_string() } else { format!("{} on it: {}", plural(n.vehicles(), "vehicle", "vehicles"), kinds.join(", ")) };
    ui.label(vehicles);
    if n.bikes + n.walkers > 0 {
        hint(ui, &format!("{} and {} along it and its pavements", plural(n.walkers, "walker", "walkers"), plural(n.bikes, "cyclist", "cyclists")));
    }
    if n.vehicles() > 0 && card.limit > 0.0 {
        let cong = n.congestion();
        let col = match cong {
            "jammed" => WARN,
            "slow" => BAD,
            "busy" => FAIR,
            _ => GOOD,
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("Moving at {:.0} km/h of {:.0} ·", n.mean_speed * 3.6, n.limit * 3.6));
            ui.label(egui::RichText::new(cong).color(col).strong()).on_hover_text("By the mean speed against what the road allows (the limit, less what a rough surface takes off): free-flowing over 75%, busy over 45%, slow under, jammed when most are standing in a queue as well.");
        });
        if n.queued > 0 {
            ui.label(egui::RichText::new(format!("{} queued, back {:.0} m from the end of the lane", plural(n.queued, "vehicle", "vehicles"), n.queue_m)).color(if n.queue_m > card.length * 0.6 { WARN } else { IVORY }));
        }
        hint(ui, &format!("{:.0} vehicles per lane-km", n.density()));
    }

    // Flow.
    let (this_hour, last_hour, today, yesterday) = card.flows();
    section(ui, "FLOW");
    egui::Grid::new(("road_flow", sid.0)).num_columns(4).spacing(egui::vec2(14.0, 2.0)).show(ui, |ui| {
        for h in ["this hour", "last hour", "today", "yesterday"] {
            ui.label(egui::RichText::new(h).small().weak());
        }
        ui.end_row();
        for v in [this_hour, last_hour, today, yesterday] {
            ui.label(egui::RichText::new(v.to_string()).strong());
        }
        ui.end_row();
    })
    .response
    .on_hover_text("Vehicles that drove onto this piece (both ways), counted as they come on.");
    if !compact {
        if let Some((h, c)) = card.peak() {
            hint(ui, &format!("Busiest hour of the last three days: {c} on day {}, {:02}:00", h / 24 + 1, h % 24));
        }
        hour_chart(ui, &card.hours, card.hour_now);
    }

    // Decay.
    let mut repair = false;
    section(ui, "CONDITION");
    match &card.decay {
        None => hint(ui, "Not the city's to mend: paths, track, bored tunnels, highway carriageways and roads past the map edge are looked after by others (and do not wear here)."),
        Some(d) => {
            let col = match d.label {
                "good" => GOOD,
                "fair" => FAIR,
                "worn" => BAD,
                _ => WARN,
            };
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(capitalise(d.label)).color(col).strong());
                ui.label(format!("· {:.0}% worn", d.wear * 100.0));
            });
            wear_bar(ui, d.wear);
            if d.speed_factor < 0.999 {
                ui.label(egui::RichText::new(format!("Rough: drivers keep to {:.0}% of the limit; {:.1}x the crashes", d.speed_factor * 100.0, d.crash_factor)).color(BAD));
            } else {
                hint(ui, &format!("Smooth enough to drive at the limit (roughness starts at {:.0}% worn)", nd_sim::road_wear::ROUGH_FROM * 100.0));
            }
            let rate = format!("{:.2}% a day ({})", d.rate * 100.0, if d.rate_measured { "measured over the last whole day" } else { "reckoned from its age and yesterday's traffic" });
            let next = match d.next {
                Some((label, days)) => format!(" · {label} in about {}", days_text(days)),
                None if d.wear >= nd_sim::road_wear::CRUMBLING => " · crumbling: as bad as it gets".to_string(),
                None => String::new(),
            };
            hint(ui, &format!("Wears {rate}{next}"));
            ui.label(format!("Resurfacing it costs {:.0}", d.repair_cost));
            match d.queue_place {
                Some(k) => {
                    let funded = if d.funding <= 0.0 { " (the maintenance budget is cut: nobody comes)" } else { "" };
                    ui.label(format!("#{k} of {} in the crews' queue{funded}", d.queue_len)).on_hover_text(format!("Priority {:.2}: wear x (0.35 + 1.3 x land value {:.2}). Dear streets are seen first.", d.priority, d.land_value));
                }
                None => hint(ui, &format!("Not in the crews' queue: they come at {:.0}% worn ({} waiting)", nd_sim::road_wear::REPAIR_AT * 100.0, d.queue_len)),
            }
            let repaired = match d.repaired_day {
                Some(day) => format!("Last {} day {}{}", if d.repaired_whole { "resurfaced" } else { "patched" }, day + 1, if d.repaired_by_order { " (your order)" } else { " by the crews" }),
                None => "No repair seen since the record began".into(),
            };
            hint(ui, &repaired);
            let can = sim.world.economy.treasury >= d.repair_cost && d.wear >= 0.005;
            let tip = if d.wear < 0.005 { "Like new already".to_string() } else if can { format!("Send a crew now: {:.0} from the treasury, booked as road maintenance", d.repair_cost) } else { "The treasury cannot cover it".to_string() };
            repair = ui.add_enabled(can, egui::Button::new(format!("Repair now ({:.0})", d.repair_cost))).on_hover_text(tip.clone()).on_disabled_hover_text(tip).clicked();
        }
    }

    // What stands on it.
    if !compact {
        let mut on = Vec::new();
        for (_, name, price) in &card.tolls {
            on.push(format!("Toll: {name} ({price:.1} a car)"));
        }
        let (arms, zebras, buttons, bridges) = card.crossings;
        if arms + zebras > 0 {
            let mut c = Vec::new();
            if arms > 0 {
                c.push(plural(arms, "junction crosswalk", "junction crosswalks"));
            }
            if zebras > 0 {
                c.push(plural(zebras, "zebra", "zebras"));
            }
            if buttons > 0 {
                c.push(format!("{buttons} with push-button lights"));
            }
            if bridges > 0 {
                c.push(plural(bridges, "footbridge", "footbridges"));
            }
            on.push(format!("Crossings: {}", c.join(", ")));
        }
        if !card.bus_stops.is_empty() {
            on.push(format!("Bus stop of {}", card.bus_stops.join(", ")));
        }
        if card.kerb_parking {
            on.push(format!("Kerb parking: {} parked", card.kerb_parked));
        } else if w.roads.segment(sid).is_street() && card.kerb_parked == 0 {
            on.push("No kerb parking".into());
        }
        if !on.is_empty() {
            section(ui, "ON THIS PIECE");
            for line in on {
                ui.label(egui::RichText::new(line).small());
            }
        }
    }
    if repair {
        return Some(match sim.world.repair_road_now(sid) {
            Ok(cost) => format!("{} resurfaced for {cost:.0}.", card.name),
            Err(e) => format!("No repair: {e}."),
        });
    }
    None
}

/// "Avenue · 2 + 2 lanes · 50 km/h · 120 m · at grade".
fn identity_line(c: &Card) -> String {
    let mut parts = vec![capitalise(c.kind)];
    match c.kind {
        "rail" => parts.push("single track".into()),
        "path" => parts.push("walkers and bikes".into()),
        _ if c.one_way => parts.push(format!("one-way, {}", plural((c.lanes_fwd + c.lanes_bwd) as u32, "lane", "lanes"))),
        _ => parts.push(format!("{} + {} lanes", c.lanes_fwd, c.lanes_bwd)),
    }
    if c.limit > 0.0 {
        parts.push(format!("{:.0} km/h", c.limit * 3.6));
    }
    parts.push(format!("{:.0} m", c.length));
    parts.push(c.level.to_string());
    parts.join(" · ")
}

fn days_text(d: f32) -> String {
    if d < 1.0 {
        "under a day".into()
    } else if d < 1.5 {
        "a day".into()
    } else if d > 999.0 {
        "over 999 days".into()
    } else {
        format!("{d:.0} days")
    }
}

fn wear_bar(ui: &mut egui::Ui, wear: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(300.0), 8.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 2.0, PLOT_BG);
    let col = if wear >= nd_sim::road_wear::CRUMBLING { WARN } else if wear >= nd_sim::road_wear::WORN { BAD } else if wear >= nd_sim::road_wear::REPAIR_AT { FAIR } else { GOOD };
    p.rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width() * wear.clamp(0.0, 1.0), r.height())), 2.0, col);
    for (at, _) in nd_sim::road_traffic::STEPS {
        let x = r.left() + r.width() * at;
        p.line_segment([egui::pos2(x, r.top()), egui::pos2(x, r.bottom())], Stroke::new(1.0, Color32::from_gray(120)));
    }
}

/// Bars of vehicles onto the piece each hour over the last three days (the
/// hour under way dimmed), midnights marked, the hour under the pointer
/// read out.
fn hour_chart(ui: &mut egui::Ui, hours: &[u16], hour_now: u32) {
    let (resp, painter) = ui.allocate_painter(egui::vec2(ui.available_width(), 90.0), egui::Sense::hover());
    let rect = resp.rect;
    painter.rect_filled(rect, 2.0, PLOT_BG);
    let small = crate::ui::theme::small();
    let tick = Color32::from_gray(170);
    let n = hours.len();
    if n == 0 || hours.iter().all(|&c| c == 0) {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "the chart fills in hour by hour", small, tick);
        return;
    }
    let max = hours.iter().copied().max().unwrap_or(1).max(1) as f32;
    let plot = egui::Rect::from_min_max(rect.min + egui::vec2(4.0, 13.0), rect.max - egui::vec2(4.0, 15.0));
    let bw = plot.width() / n as f32;
    // Absolute hour of the first bar (negative before the game began).
    let first = hour_now as i64 + 1 - n as i64;
    for (i, &c) in hours.iter().enumerate() {
        let h = first + i as i64;
        let x0 = plot.left() + i as f32 * bw;
        let fade = if i + 1 == n { 0.45 } else { 1.0 };
        if c > 0 {
            let r = egui::Rect::from_min_max(egui::pos2(x0 + 0.5, plot.bottom() - plot.height() * c as f32 / max), egui::pos2(x0 + bw - 0.5, plot.bottom()));
            painter.rect_filled(r, 0.0, GOLD.gamma_multiply(fade));
        }
        if h >= 0 && h % 24 == 0 {
            painter.line_segment([egui::pos2(x0, plot.top()), egui::pos2(x0, rect.bottom())], Stroke::new(1.0, Color32::from_gray(90)));
            painter.text(egui::pos2(x0 + 2.0, rect.bottom() - 1.0), egui::Align2::LEFT_BOTTOM, format!("day {}", h / 24 + 1), small.clone(), tick);
        }
    }
    painter.text(rect.left_top() + egui::vec2(4.0, 1.0), egui::Align2::LEFT_TOP, format!("{max:.0} an hour"), small.clone(), tick);
    painter.text(rect.right_top() + egui::vec2(-4.0, 1.0), egui::Align2::RIGHT_TOP, "last 72 h", small, tick);
    if let Some(pos) = resp.hover_pos() {
        let i = (((pos.x - plot.left()) / bw).floor().max(0.0) as usize).min(n - 1);
        let x0 = plot.left() + i as f32 * bw;
        painter.rect_stroke(egui::Rect::from_min_max(egui::pos2(x0, plot.top()), egui::pos2(x0 + bw, plot.bottom())), 0.0, Stroke::new(1.0, Color32::from_gray(150)), egui::StrokeKind::Inside);
        let h = (first + i as i64).max(0);
        let so_far = if i + 1 == n { ", so far" } else { "" };
        resp.on_hover_text(format!("Day {}, {:02}:00{so_far}\n{} vehicles onto it", h / 24 + 1, h % 24, hours[i]));
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}
