//! [pt-ux] The Alerts window: a draggable, live panel of what the toasts
//! said as it happened (city news, social news, the skyline, the HUD's own
//! messages) and the city's standing problems (the remote API's `report`
//! alerts), each with a "go" that flies the camera there.
//!
//! The feed is the toast log (`UiFlags::log`, the newest last, capped at
//! [`LOG_CAP`]); the problems are worked out again every
//! [`PROBLEMS_EVERY`] seconds, not every frame (they scan every citizen).

use bevy::prelude::Vec2;
use bevy_egui::egui;

use super::{hint, section, UiCtx, IVORY, WARN};

/// The most toasts the log keeps.
pub const LOG_CAP: usize = 200;
/// How often (real seconds) the problems are read again.
const PROBLEMS_EVERY: f64 = 2.0;

/// One toast as it happened.
#[derive(Clone, Debug, PartialEq)]
pub struct Logged {
    pub text: String,
    /// The game tick it showed at.
    pub tick: u64,
    /// Where it happened, when the writer said (city news does).
    pub at: Option<Vec2>,
}

/// Add a toast to the log: a repeat of the newest line is not logged again,
/// and the oldest go past [`LOG_CAP`].
pub fn push(log: &mut Vec<Logged>, entry: Logged) {
    if log.last().is_some_and(|l| l.text == entry.text) {
        return;
    }
    log.push(entry);
    if log.len() > LOG_CAP {
        let extra = log.len() - LOG_CAP;
        log.drain(..extra);
    }
}

/// A problem as last read.
#[derive(Clone)]
struct Problem {
    severity: u8,
    count: usize,
    message: String,
    hint: &'static str,
    at: Option<Vec2>,
}

#[derive(Clone, Default)]
struct Problems {
    at: f64,
    rows: Vec<Problem>,
}

fn problems_id() -> egui::Id {
    egui::Id::new("pt_ux_alert_problems")
}

fn severity_color(s: u8) -> egui::Color32 {
    match s {
        3 => WARN,
        2 => egui::Color32::from_rgb(235, 170, 70),
        _ => egui::Color32::from_gray(170),
    }
}

/// The window's body.
pub fn body(ui: &mut egui::Ui, p: &mut UiCtx) {
    let now = ui.input(|i| i.time);
    let w = &p.sim.world;
    let mut problems: Problems = ui.data(|d| d.get_temp(problems_id())).unwrap_or_default();
    if problems.rows.is_empty() && problems.at == 0.0 || now - problems.at > PROBLEMS_EVERY {
        problems = Problems {
            at: now,
            rows: crate::remote::city_alerts(w)
                .into_iter()
                .map(|a: crate::remote::CityAlert| Problem { severity: a.severity, count: a.count, message: a.message.clone(), hint: a.hint, at: a.at().map(|v| Vec2::new(v.x, v.y)) })
                .collect(),
        };
        ui.data_mut(|d| d.insert_temp(problems_id(), problems.clone()));
    }
    let mut fly = None;
    section(ui, "AS IT HAPPENS");
    if p.flags.log.is_empty() {
        hint(ui, "Nothing yet: news and messages land here as they pop up.");
    }
    egui::ScrollArea::vertical().id_salt("alerts_feed").max_height(300.0).auto_shrink([false, true]).show(ui, |ui| {
        for (i, l) in p.flags.log.iter().enumerate().rev().take(60) {
            let fresh = w.clock.tick.saturating_sub(l.tick) <= 60;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(super::fmt::day_time(l.tick)).small().color(super::pane::CAPTION));
                if let Some(at) = l.at {
                    if ui.small_button("go").on_hover_text("Fly there").clicked() {
                        fly = Some(at);
                    }
                }
                let text = egui::RichText::new(&l.text);
                ui.add(egui::Label::new(if fresh { text.color(IVORY) } else { text.weak() }).wrap());
            });
            if i > 0 {
                ui.add_space(1.0);
            }
        }
    });
    section(ui, "PROBLEMS NOW");
    if problems.rows.is_empty() {
        hint(ui, "No problems: the city is running smoothly.");
    }
    egui::ScrollArea::vertical().id_salt("alerts_problems").max_height(240.0).auto_shrink([false, true]).show(ui, |ui| {
    for pr in &problems.rows {
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, severity_color(pr.severity));
            if let Some(at) = pr.at {
                if ui.small_button("go").on_hover_text("Fly to the middle of the cases").clicked() {
                    fly = Some(at);
                }
            }
            let msg = if pr.message.is_empty() { format!("{} cases", pr.count) } else { pr.message.clone() };
            ui.add(egui::Label::new(egui::RichText::new(msg).color(if pr.severity >= 3 { WARN } else { IVORY })).wrap()).on_hover_text(pr.hint);
        });
    }
    });
    ui.add_space(4.0);
    hint(ui, "Drag the window by any empty part. Hover a problem for what to do about it.");
    if let Some(at) = fly {
        p.fly_to = Some(at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_skips_repeats_and_keeps_the_newest() {
        let mut log = Vec::new();
        let e = |t: &str, tick| Logged { text: t.into(), tick, at: None };
        push(&mut log, e("a", 1));
        push(&mut log, e("a", 2));
        push(&mut log, e("b", 3));
        push(&mut log, e("a", 4));
        assert_eq!(log.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["a", "b", "a"]);
        for i in 0..(LOG_CAP as u64 + 10) {
            push(&mut log, e(&format!("n{i}"), i));
        }
        assert_eq!(log.len(), LOG_CAP);
        assert_eq!(log.last().unwrap().text, format!("n{}", LOG_CAP + 9));
    }
}
