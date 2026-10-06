//! Small vector icons painted with egui's painter: one per tool, a plan of
//! each road preset (lanes, direction arrows, rails), and a glyph per land
//! use. No image files: every icon is a few shapes in a unit square scaled
//! into the rect it is given, so they stay sharp at any UI scale.

use bevy_egui::egui::{self, Color32, Painter, Pos2, Rect, Shape, Stroke};
use nd_sim::roads::RoadKind;
use nd_sim::LandUse;

use crate::tools::Tool;

/// Maps unit-square coordinates into a rect.
struct Frame {
    r: Rect,
}

impl Frame {
    fn new(r: Rect) -> Self {
        // Square, centred, so icons keep their proportions.
        let s = r.width().min(r.height());
        Self { r: Rect::from_center_size(r.center(), egui::vec2(s, s)) }
    }
    fn p(&self, x: f32, y: f32) -> Pos2 {
        egui::pos2(self.r.left() + x * self.r.width(), self.r.top() + y * self.r.height())
    }
    fn s(&self, v: f32) -> f32 {
        v * self.r.width()
    }
    fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(self.p(x0, y0), self.p(x1, y1))
    }
    fn poly(&self, pts: &[(f32, f32)], fill: Color32, stroke: Stroke) -> Shape {
        Shape::convex_polygon(pts.iter().map(|&(x, y)| self.p(x, y)).collect(), fill, stroke)
    }
    fn line(&self, pts: &[(f32, f32)], stroke: Stroke) -> Shape {
        Shape::line(pts.iter().map(|&(x, y)| self.p(x, y)).collect(), stroke)
    }
}

const ASPHALT: Color32 = Color32::from_rgb(58, 60, 72);
const MARKING: Color32 = Color32::from_rgb(235, 225, 190);
const RAIL: Color32 = Color32::from_rgb(190, 195, 205);
const SLEEPER: Color32 = Color32::from_rgb(120, 92, 62);

/// The tool's icon in `color`.
pub fn tool(painter: &Painter, rect: Rect, t: Tool, color: Color32) {
    let f = Frame::new(rect);
    let thin = Stroke::new(f.s(0.07).max(1.2), color);
    let thick = Stroke::new(f.s(0.13).max(2.0), color);
    match t {
        Tool::Inspect => {
            painter.circle_stroke(f.p(0.42, 0.42), f.s(0.25), thin);
            painter.add(f.line(&[(0.61, 0.61), (0.86, 0.86)], thick));
        }
        Tool::Road => {
            painter.add(f.poly(&[(0.36, 0.1), (0.64, 0.1), (0.9, 0.9), (0.1, 0.9)], color.gamma_multiply(0.35), thin));
            for (y0, y1) in [(0.14, 0.28), (0.4, 0.56), (0.68, 0.88)] {
                painter.add(f.line(&[(0.5, y0), (0.5, y1)], thin));
            }
        }
        Tool::Zone => {
            let cells = [(0.12, 0.12, Color32::from_rgb(90, 200, 120)), (0.52, 0.12, Color32::from_rgb(80, 150, 255)), (0.12, 0.52, Color32::from_rgb(240, 200, 70)), (0.52, 0.52, Color32::from_rgb(220, 90, 230))];
            for (x, y, c) in cells {
                painter.rect(f.rect(x, y, x + 0.36, y + 0.36), 2.0, c.gamma_multiply(0.8), Stroke::new(1.0, color), egui::StrokeKind::Inside);
            }
        }
        Tool::Edit => {
            // A pencil, point down-left.
            painter.add(f.poly(&[(0.68, 0.12), (0.88, 0.32), (0.4, 0.8), (0.2, 0.6)], color.gamma_multiply(0.5), thin));
            painter.add(f.poly(&[(0.2, 0.6), (0.4, 0.8), (0.1, 0.9)], color, Stroke::NONE));
            painter.add(f.line(&[(0.6, 0.2), (0.8, 0.4)], thin));
        }
        Tool::Bulldoze => {
            // Tracks, cab, arm and blade.
            painter.rect(f.rect(0.3, 0.66, 0.92, 0.86), f.s(0.1), color.gamma_multiply(0.35), thin, egui::StrokeKind::Inside);
            painter.rect_filled(f.rect(0.48, 0.3, 0.78, 0.62), 2.0, color.gamma_multiply(0.75));
            painter.rect_filled(f.rect(0.4, 0.46, 0.86, 0.64), 1.0, color);
            painter.add(f.line(&[(0.4, 0.56), (0.2, 0.6)], thin));
            painter.add(f.poly(&[(0.08, 0.42), (0.18, 0.42), (0.22, 0.86), (0.06, 0.86)], color, Stroke::NONE));
        }
        Tool::Buildings => {
            painter.add(Shape::convex_polygon(vec![f.p(0.6, 0.06), f.p(0.22, 0.56), f.p(0.5, 0.56)], color, Stroke::NONE));
            painter.add(Shape::convex_polygon(vec![f.p(0.5, 0.44), f.p(0.78, 0.44), f.p(0.4, 0.94)], color, Stroke::NONE));
            painter.add(Shape::convex_polygon(vec![f.p(0.36, 0.44), f.p(0.6, 0.44), f.p(0.5, 0.56), f.p(0.3, 0.56)], color, Stroke::NONE));
        }
        Tool::Transit => {
            // A bus seen from the front.
            painter.rect(f.rect(0.18, 0.08, 0.82, 0.8), f.s(0.1), color.gamma_multiply(0.35), thin, egui::StrokeKind::Inside);
            painter.rect_filled(f.rect(0.26, 0.18, 0.74, 0.46), 2.0, color.gamma_multiply(0.8));
            painter.circle_filled(f.p(0.32, 0.64), f.s(0.06), color);
            painter.circle_filled(f.p(0.68, 0.64), f.s(0.06), color);
            painter.rect_filled(f.rect(0.22, 0.8, 0.34, 0.92), 1.0, color);
            painter.rect_filled(f.rect(0.66, 0.8, 0.78, 0.92), 1.0, color);
        }
        Tool::District => {
            // A district: a painted patch with a dashed border and a flag.
            painter.rect_filled(f.rect(0.1, 0.34, 0.9, 0.9), 2.0, color.gamma_multiply(0.25));
            for (a, b) in [((0.1, 0.34), (0.9, 0.34)), ((0.9, 0.34), (0.9, 0.9)), ((0.9, 0.9), (0.1, 0.9)), ((0.1, 0.9), (0.1, 0.34))] {
                painter.add(f.line(&[a, b], thin));
            }
            painter.add(f.line(&[(0.5, 0.62), (0.5, 0.08)], thin));
            painter.add(f.poly(&[(0.5, 0.08), (0.78, 0.16), (0.5, 0.26)], color, Stroke::NONE));
        }
        Tool::Rail => {
            for y in [0.18f32, 0.38, 0.58, 0.78] {
                let k = 0.08 + 0.1 * y;
                painter.add(f.line(&[(0.5 - k - 0.14, y), (0.5 + k + 0.14, y)], Stroke::new(f.s(0.06).max(1.0), color.gamma_multiply(0.6))));
            }
            painter.add(f.line(&[(0.4, 0.06), (0.24, 0.94)], thin));
            painter.add(f.line(&[(0.6, 0.06), (0.76, 0.94)], thin));
        }
    }
}

/// A road preset in plan, running up the icon: the carriageway as wide as
/// its lanes, dashed lane lines, a centre line on two-way roads, an arrow
/// per lane in its direction; paths as a paved strip, rail as track.
pub fn road(painter: &Painter, rect: Rect, kind: RoadKind, fwd: u8, bwd: u8, color: Color32) {
    let f = Frame::new(rect);
    let s1 = Stroke::new(f.s(0.05).max(1.0), MARKING);
    match kind {
        RoadKind::Path => {
            painter.rect_filled(f.rect(0.38, 0.04, 0.62, 0.96), 1.0, Color32::from_rgb(150, 135, 110));
            for k in 0..5 {
                let y = 0.1 + k as f32 * 0.18;
                painter.add(f.line(&[(0.38, y), (0.62, y)], Stroke::new(1.0, Color32::from_rgb(110, 98, 80))));
            }
        }
        RoadKind::Rail => {
            for k in 0..6 {
                let y = 0.08 + k as f32 * 0.16;
                painter.rect_filled(f.rect(0.22, y, 0.78, y + 0.06), 0.0, SLEEPER);
            }
            painter.add(f.line(&[(0.34, 0.02), (0.34, 0.98)], Stroke::new(f.s(0.06).max(1.2), RAIL)));
            painter.add(f.line(&[(0.66, 0.02), (0.66, 0.98)], Stroke::new(f.s(0.06).max(1.2), RAIL)));
        }
        RoadKind::Street => {
            let lanes = (fwd + bwd).max(1) as f32;
            let lane_w = (0.84 / lanes).min(0.2);
            let w = lane_w * lanes;
            let x0 = 0.5 - w * 0.5;
            let curved = fwd == 1 && bwd == 0;
            if curved {
                // A ramp: a single lane peeling off to the right.
                let pts: Vec<(f32, f32)> = (0..=10).map(|i| {
                    let t = i as f32 / 10.0;
                    (0.3 + 0.45 * t * t, 0.96 - 0.92 * t)
                }).collect();
                painter.add(f.line(&pts, Stroke::new(f.s(lane_w + 0.06), ASPHALT)));
                painter.add(f.line(&pts, Stroke::new(1.0, MARKING.gamma_multiply(0.5))));
                arrow(painter, &f, 0.66, 0.24, -0.5, color);
                return;
            }
            painter.rect_filled(f.rect(x0, 0.02, x0 + w, 0.98), 1.0, ASPHALT);
            let highway = bwd == 0 && fwd >= 3;
            let edge = if highway { Color32::from_rgb(240, 200, 60) } else { MARKING };
            painter.add(f.line(&[(x0 + 0.015, 0.02), (x0 + 0.015, 0.98)], Stroke::new(1.0, edge)));
            painter.add(f.line(&[(x0 + w - 0.015, 0.02), (x0 + w - 0.015, 0.98)], Stroke::new(1.0, edge)));
            for i in 1..(fwd + bwd) {
                let x = x0 + lane_w * i as f32;
                if i == bwd && bwd > 0 {
                    // Centre line between the directions.
                    painter.add(f.line(&[(x - 0.012, 0.02), (x - 0.012, 0.98)], Stroke::new(1.0, Color32::from_rgb(240, 200, 60))));
                    painter.add(f.line(&[(x + 0.012, 0.02), (x + 0.012, 0.98)], Stroke::new(1.0, Color32::from_rgb(240, 200, 60))));
                } else {
                    for k in 0..4 {
                        let y = 0.06 + k as f32 * 0.25;
                        painter.add(f.line(&[(x, y), (x, y + 0.12)], s1));
                    }
                }
            }
            // Backward lanes on the left run down, forward lanes up.
            for i in 0..(fwd + bwd) {
                let x = x0 + lane_w * (i as f32 + 0.5);
                let up = i >= bwd;
                arrow(painter, &f, x, if up { 0.42 } else { 0.58 }, if up { -1.0 } else { 1.0 }, color);
            }
        }
    }
}

/// A small arrowhead at (x, y) pointing up (`dir < 0`) or down; `dir`'s
/// size tilts it for ramps.
/// A roundabout seen from above: four arms, the ring with a dashed lane
/// line, a green island, and an arrow showing the way round (anticlockwise
/// on screen).
pub fn roundabout(painter: &Painter, rect: Rect, color: Color32) {
    let f = Frame::new(rect);
    let c = f.p(0.5, 0.5);
    let arm = Stroke::new(f.s(0.2), ASPHALT);
    for (x, y) in [(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)] {
        painter.add(f.line(&[(0.5, 0.5), (x, y)], arm));
    }
    painter.circle_stroke(c, f.s(0.3), Stroke::new(f.s(0.17), ASPHALT));
    for k in 0..8 {
        let a0 = k as f32 / 8.0 * std::f32::consts::TAU;
        let a1 = a0 + 0.35;
        let p = |a: f32| egui::pos2(c.x + a.cos() * f.s(0.3), c.y + a.sin() * f.s(0.3));
        painter.line_segment([p(a0), p(a1)], Stroke::new(1.0, MARKING.gamma_multiply(0.7)));
    }
    painter.circle_filled(c, f.s(0.18), Color32::from_rgb(70, 140, 80));
    painter.circle_filled(c, f.s(0.06), Color32::from_rgb(40, 95, 50));
    // Anticlockwise on screen: along the top edge from right to left.
    let a = -std::f32::consts::FRAC_PI_2 - 0.15;
    let tip = egui::pos2(c.x + a.cos() * f.s(0.3), c.y + a.sin() * f.s(0.3));
    let d = egui::vec2(-1.0, -0.15).normalized() * f.s(0.12);
    let n = egui::vec2(-d.y, d.x);
    painter.add(Shape::convex_polygon(vec![tip + d, tip - d * 0.4 + n * 0.8, tip - d * 0.4 - n * 0.8], color, Stroke::NONE));
}

fn arrow(painter: &Painter, f: &Frame, x: f32, y: f32, dir: f32, color: Color32) {
    let d = dir.signum() * 0.09;
    let tilt = if dir.abs() < 1.0 { 0.06 } else { 0.0 };
    painter.add(Shape::convex_polygon(vec![f.p(x + tilt, y + d), f.p(x - 0.07, y - d * 0.4), f.p(x + 0.07, y - d * 0.4)], color, Stroke::NONE));
    painter.add(f.line(&[(x, y - d * 0.4), (x - tilt * 0.5, y - d * 1.6)], Stroke::new(f.s(0.04).max(1.0), color)));
}

/// A glyph for a land use, in its zone colour: a house, a shopfront with an
/// awning, a factory with a stack, a star, a columned hall, a tree.
pub fn land_use(painter: &Painter, rect: Rect, lu: LandUse, color: Color32) {
    let f = Frame::new(rect);
    let st = Stroke::new(f.s(0.07).max(1.0), color);
    let fill = color.gamma_multiply(0.45);
    match lu {
        LandUse::Residential => {
            painter.add(f.poly(&[(0.5, 0.1), (0.9, 0.48), (0.1, 0.48)], color, Stroke::NONE));
            painter.rect(f.rect(0.2, 0.48, 0.8, 0.9), 0.0, fill, st, egui::StrokeKind::Inside);
            painter.rect_filled(f.rect(0.42, 0.62, 0.58, 0.9), 0.0, color);
        }
        LandUse::Commercial => {
            painter.rect(f.rect(0.12, 0.38, 0.88, 0.9), 0.0, fill, st, egui::StrokeKind::Inside);
            for k in 0..4 {
                let x = 0.1 + k as f32 * 0.2;
                painter.add(f.poly(&[(x, 0.16), (x + 0.2, 0.16), (x + 0.2, 0.36), (x + 0.1, 0.44), (x, 0.36)], if k % 2 == 0 { color } else { fill }, Stroke::NONE));
            }
            painter.rect_filled(f.rect(0.24, 0.56, 0.5, 0.78), 0.0, color);
        }
        LandUse::Industrial => {
            painter.rect_filled(f.rect(0.08, 0.52, 0.74, 0.9), 0.0, fill);
            painter.add(f.poly(&[(0.08, 0.52), (0.3, 0.38), (0.3, 0.52)], fill, Stroke::NONE));
            painter.add(f.poly(&[(0.3, 0.52), (0.52, 0.38), (0.52, 0.52)], fill, Stroke::NONE));
            painter.add(f.poly(&[(0.52, 0.52), (0.74, 0.38), (0.74, 0.52)], fill, Stroke::NONE));
            painter.add(f.line(&[(0.08, 0.9), (0.08, 0.52), (0.3, 0.38), (0.3, 0.52), (0.52, 0.38), (0.52, 0.52), (0.74, 0.38), (0.74, 0.9), (0.08, 0.9)], st));
            painter.rect_filled(f.rect(0.76, 0.14, 0.88, 0.9), 0.0, color);
            painter.circle_filled(f.p(0.84, 0.08), f.s(0.05), color.gamma_multiply(0.6));
        }
        LandUse::Entertainment => {
            let pts: Vec<Pos2> = (0..10).map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                let r = if i % 2 == 0 { 0.42 } else { 0.18 };
                f.p(0.5 + a.cos() * r, 0.54 + a.sin() * r)
            }).collect();
            // A star is not convex: draw it as a fan of triangles.
            for i in 0..10 {
                painter.add(Shape::convex_polygon(vec![f.p(0.5, 0.54), pts[i], pts[(i + 1) % 10]], color, Stroke::NONE));
            }
        }
        LandUse::Civic => {
            painter.add(f.poly(&[(0.5, 0.08), (0.92, 0.3), (0.08, 0.3)], color, Stroke::NONE));
            for k in 0..4 {
                let x = 0.18 + k as f32 * 0.2;
                painter.rect_filled(f.rect(x, 0.36, x + 0.08, 0.78), 0.0, color);
            }
            painter.rect_filled(f.rect(0.08, 0.8, 0.92, 0.9), 0.0, color);
        }
        LandUse::Park => {
            painter.rect_filled(f.rect(0.45, 0.56, 0.55, 0.92), 0.0, color.gamma_multiply(0.7));
            painter.circle_filled(f.p(0.5, 0.36), f.s(0.28), color);
            painter.circle_filled(f.p(0.3, 0.5), f.s(0.16), color);
            painter.circle_filled(f.p(0.7, 0.5), f.s(0.16), color);
        }
    }
}

/// A selectable label with an icon in front of the text, painted by
/// `icon(painter, rect, colour)`. Looks and behaves like
/// `ui.selectable_label`.
pub fn selectable(ui: &mut egui::Ui, selected: bool, text: impl Into<egui::WidgetText>, icon: impl FnOnce(&Painter, Rect, Color32)) -> egui::Response {
    let galley = text.into().into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, egui::TextStyle::Button);
    let side = 18.0f32.max(galley.size().y);
    let pad = ui.spacing().button_padding;
    let size = egui::vec2(pad.x + side + 4.0 + galley.size().x + pad.x, side + pad.y);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact_selectable(&resp, selected);
        if selected || resp.hovered() || resp.highlighted() || resp.has_focus() {
            ui.painter().rect(rect, v.corner_radius, v.weak_bg_fill, v.bg_stroke, egui::StrokeKind::Inside);
        }
        let icon_rect = Rect::from_min_size(egui::pos2(rect.left() + pad.x, rect.center().y - side * 0.5), egui::vec2(side, side));
        icon(ui.painter(), icon_rect.shrink(1.0), v.text_color());
        let text_pos = egui::pos2(icon_rect.right() + 4.0, rect.center().y - galley.size().y * 0.5);
        ui.painter().galley(text_pos, galley, v.text_color());
    }
    resp
}
