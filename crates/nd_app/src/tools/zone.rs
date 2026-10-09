//! The Zone tool: polygons into the project, or parcels painted with the
//! brush or a marquee as they are dragged; the strokes, and the previews.

use bevy::prelude::*;
use nd_sim::{Gesture, Vec2 as SVec2};

use super::input::Frame;
use super::edit::nearest_gesture;
use super::{Preview, ToolState};
use crate::overlay::shapes::flat_at;
use crate::selection::Selection;
use crate::sim::Sim;
use nd_sim::LandUse;
use super::{PaintStroke, ZoneMode};

/// The marquee from `a` to `b` as a rectangle square to the camera (its
/// yaw), so it looks like the box the player dragged on screen.
pub fn marquee_rect(a: SVec2, b: SVec2, yaw: f32) -> Vec<SVec2> {
    let (s, c) = yaw.sin_cos();
    let right = SVec2::new(c, -s);
    let fwd = SVec2::new(s, c);
    let d = b - a;
    vec![a, a + right * d.dot(right), b, a + fwd * d.dot(fwd)]
}

/// One stroke step: paint (or erase) the parcels in `ids`, note the ones
/// that refused, and keep the stroke's gesture growing.
pub(super) fn paint_step(sim: &mut Sim, tools: &mut ToolState, ids: &[usize]) {
    let Some(mut stroke) = tools.stroke.take() else { return };
    let lu = tools.paint_use(stroke.erase);
    if let Some(lu) = lu {
        for &i in ids {
            let refusal = if tools.painting_beach() && !stroke.erase { sim.world.beach_refusal(i) } else { sim.world.parcel_refusal(i, Some(lu)) };
            if let Some(why) = refusal {
                stroke.refused.insert(i, why);
            }
        }
    }
    let industry = if stroke.erase { None } else { tools.paint_industry() };
    let (g, n) = sim.world.paint_parcels_with(ids, lu, industry, stroke.gesture);
    stroke.gesture = g;
    stroke.painted += n;
    tools.stroke = Some(stroke);
}

/// What a finished stroke did, for the status line.
pub(super) fn stroke_status(stroke: &PaintStroke, lu: Option<LandUse>) -> String {
    let what = lu.map_or("unzoned".to_string(), |l| format!("zoned {}", l.label()));
    let mut out = format!("{} parcel{} {what}.", stroke.painted, if stroke.painted == 1 { "" } else { "s" });
    if !stroke.refused.is_empty() {
        let mut reasons: Vec<&str> = stroke.refused.values().copied().collect();
        reasons.sort();
        reasons.dedup();
        out.push_str(&format!(" {} refused ({}).", stroke.refused.len(), reasons.join("; ")));
    }
    if stroke.painted > 0 {
        out.push_str(" Ctrl+Z undoes the stroke, Ctrl+Enter implements.");
    }
    out
}

/// Polygon points, or a brush or marquee stroke.
pub(super) fn input(f: &Frame, sim: &mut Sim, tools: &mut ToolState, _selection: &mut Selection) {
    let Frame { ctrl, click, ground, finish, right_down, yaw, buttons, plan, .. } = *f;
    tools.hover_parcel = ground.and_then(|p| sim.world.parcels.at(p));
    tools.hover_gesture = ground.and_then(|p| nearest_gesture(plan, p)).and_then(|(id, _)| match plan.gestures.get(&id) {
        Some(Gesture::Zone { .. }) => Some(id),
        _ => None,
    });
    match tools.zone_mode {
        ZoneMode::Polygon => {
            if click && ctrl {
                if let Some(id) = tools.hover_gesture {
                    if sim.world.repaint_zone(id, tools.land_use) {
                        tools.status = format!("Zone repainted as {}. Ctrl+Enter implements.", tools.land_use.label());
                    }
                }
            } else if click {
                if let Some(p) = ground {
                    tools.points.push(p);
                }
            }
            if finish {
                if tools.points.len() >= 3 {
                    let industry = tools.paint_industry();
                    sim.world.add_zone_gesture(tools.points.clone(), tools.land_use, industry);
                    tools.status = format!("{} zone added to project: the parcels whose centre it holds take it. Ctrl+Enter implements.", tools.land_use.label());
                }
                tools.points.clear();
            }
        }
        ZoneMode::Brush | ZoneMode::Marquee => {
            // Left paints the selected use (or unzones with Unzone
            // picked), right always erases back to unzoned.
            if tools.stroke.is_none() {
                let start = if click { Some((MouseButton::Left, tools.unzone)) } else if right_down { Some((MouseButton::Right, true)) } else { None };
                if let (Some((button, erase)), Some(p)) = (start, ground) {
                    tools.stroke = Some(PaintStroke { button, erase, gesture: None, painted: 0, refused: Default::default(), from: Some(p) });
                }
            }
            let Some(stroke) = tools.stroke.clone() else { return };
            let released = !buttons.pressed(stroke.button);
            if tools.zone_mode == ZoneMode::Brush {
                if let Some(p) = ground {
                    let ids = sim.world.parcels.in_circle(p, tools.brush_radius);
                    paint_step(sim, tools, &ids);
                }
            } else if released {
                if let (Some(a), Some(b)) = (stroke.from, ground) {
                    let rect = marquee_rect(a, b, yaw);
                    let ids = sim.world.parcels.in_polygon(&rect);
                    paint_step(sim, tools, &ids);
                }
            }
            if released {
                if let Some(done) = tools.stroke.take() {
                    tools.status = stroke_status(&done, tools.paint_use(done.erase));
                }
            }
        }
    }
}

/// Brush and marquee: the circle or the box, and the parcels under it
/// green (they take the use), red (they refuse) or amber (erasing).
pub(super) fn paint_preview(gizmos: &mut Gizmos, p: &Preview) {
    let Preview { tools, cursor, sim, .. } = *p;
    let lift = |q: SVec2| super::lift(sim, q);
    let (cyan, red) = (super::CYAN, super::RED);
    let ok = Color::linear_rgb(0.3, 2.5, 0.6);
    let erase_col = Color::linear_rgb(2.2, 1.2, 0.4);
    let erasing = tools.stroke.as_ref().map_or(tools.unzone, |s| s.erase);
    let lu = tools.paint_use(erasing);
    let outline = |gizmos: &mut Gizmos, poly: &[SVec2], col: Color| {
        let mut pts: Vec<Vec3> = poly.iter().map(|&p| lift(p)).collect();
        if pts.len() >= 3 {
            if let Some(&first) = pts.first() { pts.push(first); }
            gizmos.linestrip(pts, col);
        }
    };
    let parcels = &sim.world.parcels;
    let mark = |gizmos: &mut Gizmos, ids: &[usize]| {
        for &i in ids.iter().take(400) {
            let Some(p) = parcels.parcels.get(i) else { continue };
            let col = match lu {
                Some(l) if sim.world.parcel_refusal(i, Some(l)).is_none() => ok,
                Some(_) => red,
                None => erase_col,
            };
            outline(gizmos, &p.polygon, col);
        }
    };
    if let Some(c) = cursor.ground {
        if tools.zone_mode == ZoneMode::Brush {
            gizmos.circle(flat_at(lift(c)), tools.brush_radius, if erasing { erase_col } else { cyan });
            mark(gizmos, &parcels.in_circle(c, tools.brush_radius));
        } else if let Some(a) = tools.stroke.as_ref().and_then(|s| s.from) {
            let yaw = p.yaw;
            let rect = marquee_rect(a, c, yaw);
            outline(gizmos, &rect, if erasing { erase_col } else { cyan });
            mark(gizmos, &parcels.in_polygon(&rect));
        } else if let Some(i) = tools.hover_parcel {
            mark(gizmos, &[i]);
        }
    }
}

/// The polygon so far, closed through the pointer.
pub(super) fn polygon_preview(gizmos: &mut Gizmos, p: &Preview) {
    let Preview { tools, cursor, sim, .. } = *p;
    let lift = |q: SVec2| super::lift(sim, q);
    let cyan = super::CYAN;
    let mut ctrl: Vec<SVec2> = tools.points.clone();
    if let Some(c) = cursor.ground {
        gizmos.circle(flat_at(lift(c)), 3.0, cyan);
        ctrl.push(c);
    }
    for &p in &tools.points {
        gizmos.circle(flat_at(lift(p)), 2.0, cyan);
    }
    let mut pts: Vec<Vec3> = ctrl.iter().map(|&p| lift(p)).collect();
    if pts.len() >= 2 {
        if pts.len() >= 3 {
            if let Some(&first) = pts.first() { pts.push(first); }
        }
        gizmos.linestrip(pts, cyan);
    }
}
