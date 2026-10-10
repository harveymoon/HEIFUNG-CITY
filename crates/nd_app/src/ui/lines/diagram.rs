//! [lines] The horizontal stop diagram of one line, bus or metro: the
//! stops as nodes along a track in the line's colour (names slanted above,
//! the time of each leg under it, riders on and off under each stop), the vehicles on
//! the track where they are, and under it a little box per vehicle (its
//! load, the next stop, how late it runs, what it is doing) with an arrow
//! up to where it is. A loop ends in a faint copy of the first stop.
//!
//! While a bus line is edited every leg has a "+" on the track: it picks the
//! slot the next stop clicked on the map goes into.
//!
//! The drawing is data in, a click out ([`Track`], [`Clicked`]); where the
//! boxes go ([`place_boxes`]) is a plain function, tested below.

use bevy_egui::egui::{self, Color32, Pos2, Rect, Sense, Stroke};

use crate::ui::theme::{GOLD, IVORY, PLOT_BG, WARN};

/// One stop.
pub struct Node {
    pub name: String,
    /// Boarded and alighted today.
    pub on: u32,
    pub off: u32,
    /// The tooltip (yesterday's figures and the like).
    pub tip: String,
    pub picked: bool,
}

/// The leg from one stop to the next.
pub struct Leg {
    /// Over the track: its time.
    pub text: String,
    /// No road: drawn red.
    pub broken: bool,
}

/// One vehicle on the line.
pub struct Vehicle {
    /// Where it is, in stops along the track: `2.4` is 40% of the way from
    /// the third stop to the fourth; on a loop `n - 1 .. n` is the way home
    /// to the first. `None`: not on the track (leaving or going back to the
    /// depot), boxed at the end without an arrow.
    pub at: Option<f32>,
    pub title: String,
    /// What it is doing, in its colour.
    pub state: (String, Color32),
    /// Riders aboard and room.
    pub load: (usize, usize),
    /// Further lines (next stop, delay), each in its colour.
    pub rows: Vec<(String, Color32)>,
    pub picked: bool,
}

pub struct Track {
    pub id: egui::Id,
    pub color: Color32,
    pub nodes: Vec<Node>,
    /// One per leg: `n` on a loop, `n - 1` end to end.
    pub legs: Vec<Leg>,
    pub looped: bool,
    pub vehicles: Vec<Vehicle>,
    /// Show the "+" slots (bus line edit mode) and which one is picked.
    pub editing: bool,
    pub slot: Option<usize>,
}

/// What was clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clicked {
    Stop(usize),
    Vehicle(usize),
    /// The "+" between stops `i - 1` and `i` (`i == n`: after the last).
    Slot(usize),
}

const MARGIN_L: f32 = 22.0;
const MARGIN_R: f32 = 84.0;
const MIN_SPACING: f32 = 64.0;
const NAMES_H: f32 = 74.0;
const BOX_W: f32 = 128.0;
const BOX_GAP: f32 = 6.0;
const ROW_H: f32 = 13.0;

/// Left edges for boxes `w` wide that want to sit centred under `targets`
/// (x, any order), left to right in the targets' order, `gap` apart, none
/// left of `left`: each as near its target as the ones before it allow.
pub fn place_boxes(targets: &[f32], w: f32, gap: f32, left: f32) -> Vec<f32> {
    let mut order: Vec<usize> = (0..targets.len()).collect();
    order.sort_by(|a, b| targets[*a].total_cmp(&targets[*b]).then(a.cmp(b)));
    let mut out = vec![0.0; targets.len()];
    let mut next = left;
    for i in order {
        let x = (targets[i] - w / 2.0).max(next);
        out[i] = x;
        next = x + w + gap;
    }
    out
}

/// Draw the diagram; what was clicked, if anything.
pub fn show(ui: &mut egui::Ui, t: &Track) -> Option<Clicked> {
    let n = t.nodes.len();
    if n == 0 {
        return None;
    }
    let cols = if t.looped { n } else { n.saturating_sub(1) }.max(1);
    let avail = ui.available_width();
    // Edit mode puts a "+" on each leg: a little more room.
    let spacing = ((avail - MARGIN_L - MARGIN_R) / cols as f32).max(if t.editing { MIN_SPACING + 14.0 } else { MIN_SPACING });
    let x_of = |u: f32| MARGIN_L + u * spacing;
    // The boxes: on-track ones under their vehicle, off-track ones after.
    let track_end = x_of(cols as f32);
    let targets: Vec<f32> = t.vehicles.iter().map(|v| v.at.map_or(track_end + BOX_W, |u| x_of(u))).collect();
    let lefts = place_boxes(&targets, BOX_W, BOX_GAP, 4.0);
    let rows = t.vehicles.iter().map(|v| v.rows.len()).max().unwrap_or(0);
    let box_h = 8.0 + ROW_H * (2 + rows) as f32 + 8.0;
    let width = (track_end + MARGIN_R).max(lefts.iter().map(|l| l + BOX_W + 4.0).fold(0.0, f32::max));
    let track_y = NAMES_H + 8.0;
    let boxes_y = track_y + 52.0;
    let height = if t.vehicles.is_empty() { track_y + 40.0 } else { boxes_y + box_h + 4.0 };
    let mut clicked = None;
    egui::ScrollArea::horizontal().id_salt(t.id.with("scroll")).show(ui, |ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
        let o = rect.min;
        let at = |x: f32, y: f32| Pos2::new(o.x + x, o.y + y);
        let painter = ui.painter_at(rect);
        let small = crate::ui::theme::small();
        // The track, leg by leg.
        for (i, leg) in t.legs.iter().enumerate() {
            let (a, b) = (at(x_of(i as f32), track_y), at(x_of(i as f32 + 1.0), track_y));
            painter.line_segment([a, b], Stroke::new(5.0, if leg.broken { WARN } else { t.color }));
            let mid = Pos2::new((a.x + b.x) / 2.0, track_y + o.y);
            painter.text(mid + egui::vec2(0.0, 9.0), egui::Align2::CENTER_TOP, &leg.text, small.clone(), if leg.broken { WARN } else { Color32::from_gray(150) });
        }
        // The way home on a loop: a faint first stop at the end.
        if t.looped {
            let p = at(x_of(n as f32), track_y);
            painter.circle(p, 5.5, Color32::from_rgb(12, 14, 24), Stroke::new(2.0, t.color.gamma_multiply(0.5)));
            painter.text(p + egui::vec2(0.0, 22.0), egui::Align2::CENTER_TOP, "to 1", small.clone(), Color32::from_gray(120));
        }
        // The stops.
        for (i, s) in t.nodes.iter().enumerate() {
            let p = at(x_of(i as f32), track_y);
            let r = if s.picked { 8.5 } else { 7.0 };
            let resp = ui.interact(Rect::from_center_size(p, egui::vec2(20.0, 20.0)), t.id.with(("stop", i)), Sense::click());
            let ring = if s.picked { IVORY } else if resp.hovered() { GOLD } else { t.color };
            painter.circle(p, r, Color32::from_rgb(12, 14, 24), Stroke::new(2.5, ring));
            painter.text(p, egui::Align2::CENTER_CENTER, format!("{}", i + 1), crate::ui::theme::display(9.5), IVORY);
            // The name, slanted up to the right.
            let name: String = if s.name.chars().count() > 22 { s.name.chars().take(21).chain(std::iter::once('…')).collect() } else { s.name.clone() };
            let galley = painter.layout_no_wrap(name, small.clone(), if s.picked { GOLD } else { IVORY });
            painter.add(egui::epaint::TextShape::new(p + egui::vec2(4.0, -13.0), galley, IVORY).with_angle(-0.62));
            // Riders on and off today.
            if s.on + s.off > 0 {
                painter.text(p + egui::vec2(0.0, 22.0), egui::Align2::CENTER_TOP, format!("+{} −{}", s.on, s.off), small.clone(), Color32::from_gray(150));
            }
            if resp.on_hover_text(&s.tip).clicked() {
                clicked = Some(Clicked::Stop(i));
            }
        }
        // The edit slots: a "+" on every leg.
        if t.editing {
            for i in 0..t.legs.len() {
                let p = at(x_of(i as f32 + 0.5), track_y);
                let slot = i + 1;
                let resp = ui.interact(Rect::from_center_size(p, egui::vec2(16.0, 16.0)), t.id.with(("slot", i)), Sense::click());
                let on = t.slot == Some(slot);
                let fill = if on { GOLD } else if resp.hovered() { Color32::from_gray(70) } else { Color32::from_rgb(12, 14, 24) };
                painter.circle(p, 6.5, fill, Stroke::new(1.5, if on { IVORY } else { GOLD }));
                painter.text(p, egui::Align2::CENTER_CENTER, "+", crate::ui::theme::display(12.0), if on { Color32::BLACK } else { GOLD });
                let tip = if slot == n { format!("Put the next stop you click on the map after stop {n}, before the line comes round to 1") } else { format!("Put the next stop you click on the map between stops {} and {}", slot, slot + 1) };
                if resp.on_hover_text(tip).clicked() {
                    clicked = Some(Clicked::Slot(slot));
                }
            }
        }
        // The vehicles: a marker on the track, a box below, an arrow between.
        for (k, v) in t.vehicles.iter().enumerate() {
            let left = lefts[k];
            let b = Rect::from_min_size(at(left, boxes_y), egui::vec2(BOX_W, box_h));
            let resp = ui.interact(b, t.id.with(("vehicle", k)), Sense::click());
            let edge = if v.picked { IVORY } else if resp.hovered() { GOLD } else { t.color.gamma_multiply(0.8) };
            if let Some(u) = v.at {
                let m = at(x_of(u), track_y);
                let marker = Rect::from_center_size(m, egui::vec2(15.0, 9.0));
                painter.rect_filled(marker, 2.0, IVORY);
                painter.rect_stroke(marker, 2.0, Stroke::new(1.5, t.color), egui::StrokeKind::Outside);
                let from = Pos2::new(b.center().x, b.top() - 1.0);
                let to = Pos2::new(m.x, m.y + 7.0);
                painter.arrow(from, to - from, Stroke::new(1.2, edge));
            }
            painter.rect_filled(b, 4.0, PLOT_BG);
            painter.rect_stroke(b, 4.0, Stroke::new(if v.picked { 2.0 } else { 1.0 }, edge), egui::StrokeKind::Inside);
            let mut y = b.top() + 6.0;
            let x = b.left() + 7.0;
            painter.text(Pos2::new(x, y), egui::Align2::LEFT_TOP, &v.title, crate::ui::theme::display(12.0), IVORY);
            painter.text(Pos2::new(b.right() - 7.0, y), egui::Align2::RIGHT_TOP, &v.state.0, small.clone(), v.state.1);
            y += ROW_H + 1.0;
            // The load: a bar and the count.
            let (aboard, room) = v.load;
            let frac = if room > 0 { (aboard as f32 / room as f32).clamp(0.0, 1.0) } else { 0.0 };
            let bar = Rect::from_min_size(Pos2::new(x, y + 3.0), egui::vec2(48.0, 6.0));
            painter.rect_filled(bar, 2.0, Color32::from_gray(45));
            painter.rect_filled(Rect::from_min_size(bar.min, egui::vec2(bar.width() * frac, bar.height())), 2.0, if frac > 0.9 { WARN } else { t.color });
            painter.text(Pos2::new(bar.right() + 6.0, y), egui::Align2::LEFT_TOP, format!("{aboard}/{room}"), small.clone(), IVORY);
            y += ROW_H;
            for (text, col) in &v.rows {
                let galley = painter.layout_no_wrap(text.clone(), small.clone(), *col);
                let clip = Rect::from_min_size(Pos2::new(x, y), egui::vec2(BOX_W - 12.0, ROW_H + 2.0));
                painter.with_clip_rect(clip.intersect(rect)).galley(Pos2::new(x, y), galley, *col);
                y += ROW_H;
            }
            if resp.on_hover_text("Select it (and follow it with the inspector)").clicked() {
                clicked = Some(Clicked::Vehicle(k));
            }
        }
    });
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lone_box_sits_centred_under_its_vehicle() {
        assert_eq!(place_boxes(&[200.0], 100.0, 6.0, 0.0), vec![150.0]);
        // ...but never off the left edge.
        assert_eq!(place_boxes(&[10.0], 100.0, 6.0, 4.0), vec![4.0]);
    }

    #[test]
    fn boxes_never_overlap_and_keep_the_vehicles_order() {
        // Three buses bunched together, given out of order.
        let t = [310.0, 300.0, 305.0];
        let l = place_boxes(&t, 100.0, 6.0, 0.0);
        let mut by_target: Vec<(f32, f32)> = t.iter().copied().zip(l.iter().copied()).collect();
        by_target.sort_by(|a, b| a.0.total_cmp(&b.0));
        for w in by_target.windows(2) {
            assert!(w[1].1 >= w[0].1 + 106.0 - 1e-3, "{by_target:?}");
        }
        assert_eq!(l[1], 250.0, "the first along the track keeps its place");
        // Far apart: each under its own.
        assert_eq!(place_boxes(&[100.0, 400.0], 100.0, 6.0, 0.0), vec![50.0, 350.0]);
    }
}
