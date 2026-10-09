//! The Edit tool: pick a road to upgrade, drag, insert and delete control
//! points, open the Intersection editor on a junction; its preview (every
//! control point, the hovered and selected gestures, lane arrows).

use bevy::prelude::*;
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, Vec2 as SVec2};

use super::input::Frame;
use super::{Preview, ToolState};
use crate::overlay::shapes::flat_at;
use crate::selection::Selection;
use crate::sim::Sim;
use nd_sim::roads::project_on_segment;
use nd_sim::zoning::point_in_polygon;
use nd_sim::{GestureId, Plan};

use super::POINT_PICK_RADIUS;

/// Control points of different gestures closer than this are one joint.
const SAME_POINT: f32 = 0.5;

/// The control point under the cursor. Where several gestures share it (a
/// road attached to another's end), the one whose legs from it run nearest
/// the cursor wins, then the newest: clicking a ramp's end where it meets
/// a highway must pick the ramp, not the highway (picking the older road
/// there turned the whole highway into a ramp when the player then picked
/// the Ramp preset in the upgrade panel).
pub(super) fn nearest_control_point(plan: &Plan, p: SVec2) -> Option<(GestureId, usize)> {
    let mut best: Option<(f32, f32, GestureId, usize)> = None;
    for (id, g) in &plan.gestures {
        let pts = g.points();
        for (i, &q) in pts.iter().enumerate() {
            let d = p.distance(q);
            if d >= POINT_PICK_RADIUS {
                continue;
            }
            let legs = [i.checked_sub(1), (i + 1 < pts.len()).then_some(i + 1)];
            let leg = legs.into_iter().flatten().map(|j| p.distance(project_on_segment(p, q, pts[j]).1)).fold(f32::INFINITY, f32::min);
            // Later gestures win ties: the map iterates them oldest first.
            let better = match best {
                None => true,
                Some((bd, bl, ..)) if (d - bd).abs() <= SAME_POINT => leg <= bl,
                Some((bd, ..)) => d < bd,
            };
            if better {
                best = Some((d, leg, *id, i));
            }
        }
    }
    best.map(|(_, _, id, i)| (id, i))
}

/// Gesture under the cursor: a road whose polyline is close, or a zone that
/// contains the point. Returns the segment index for roads so points can be
/// inserted there.
pub(super) fn nearest_gesture(plan: &Plan, p: SVec2) -> Option<(GestureId, usize)> {
    let mut best: Option<(f32, GestureId, usize)> = None;
    for (id, g) in &plan.gestures {
        match g {
            Gesture::Road { points, lanes_fwd, lanes_bwd, curved, kind, .. } => {
                let hw = if *kind != RoadKind::Street { 4.0 } else { (*lanes_fwd + *lanes_bwd) as f32 * nd_sim::roads::LANE_WIDTH * 0.5 + 3.0 };
                let built = g.road_polyline();
                for (i, w) in built.windows(2).enumerate() {
                    let (_, q) = project_on_segment(p, w[0], w[1]);
                    let d = p.distance(q);
                    // `<=`: at a joint the newer road (the one attached) wins.
                    if d < hw && best.map_or(true, |b| d <= b.0) {
                        // Map the built leg back to the control leg for insertion.
                        let ctrl = if *curved {
                            let mid = (w[0] + w[1]) * 0.5;
                            (0..points.len().saturating_sub(1)).min_by(|&a, &b| {
                                let da = project_on_segment(mid, points[a], points[a + 1]).1.distance(mid);
                                let db = project_on_segment(mid, points[b], points[b + 1]).1.distance(mid);
                                da.total_cmp(&db)
                            }).unwrap_or(0)
                        } else {
                            i
                        };
                        best = Some((d, *id, ctrl));
                    }
                }
            }
            Gesture::Zone { polygon, .. } => {
                if point_in_polygon(p, polygon) {
                    let d = 1000.0;
                    if best.map_or(true, |b| d < b.0) {
                        best = Some((d, *id, 0));
                    }
                }
            }
            // Paint strokes have no shape to pick: undo or the eraser takes them back.
            Gesture::ParcelZone { .. } => {}
            Gesture::Roundabout { center, radius, lanes } => {
                let ring = nd_sim::roads::Ring::new(*center, *radius, *lanes);
                let d = (p.distance(ring.center) - ring.radius).abs();
                if d < ring.half_width() + 2.0 && best.map_or(true, |b| d < b.0) {
                    best = Some((d, *id, 0));
                }
            }
        }
    }
    best.map(|(_, id, i)| (id, i))
}

/// Hover, drag, click, Shift+click and Delete.
pub(super) fn input(f: &Frame, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection) {
    let Frame { ctrl, click, ground, shift_click, buttons, plan, .. } = *f;
    let map_key = |k: KeyCode| f.map_key(k);
    tools.hover_point = ground.and_then(|p| nearest_control_point(plan, p));
    tools.hover_gesture = ground.and_then(|p| nearest_gesture(plan, p)).map(|(id, _)| id);

    if let Some((id, idx)) = tools.drag {
        if let (Some(p), Some(mut g)) = (ground, plan.gestures.get(&id).cloned()) {
            if idx < g.points().len() {
                let before = g.clone();
                g.move_point(idx, p);
                let release = !buttons.pressed(MouseButton::Left);
                drag_frame(&mut sim.world, id, &before, g, &mut tools.drag_moved);
                if release {
                    tools.drag = None;
                    tools.status = "Control point moved".into();
                }
            } else {
                tools.drag = None;
            }
        } else {
            tools.drag = None;
        }
    } else if click {
        // [tools] A junction or a crosswalk opens the Intersection
        // editor (Ctrl+click still grabs a control point there).
        let target = ground.filter(|_| !ctrl).and_then(|p| crate::ui::junction::target_at(&sim.world, p));
        if let Some(at) = target {
            tools.junction_editor = crate::ui::junction::EditorState::open(&sim.world, at);
            tools.status = "Intersection editor: lanes, left turns, signals and crosswalks.".into();
        } else if let Some(hp) = tools.hover_point {
            tools.drag = Some(hp);
            tools.drag_moved = false;
            tools.selected_gesture = Some(hp.0);
        } else {
            tools.selected_gesture = tools.hover_gesture;
            if tools.selected_gesture.is_some() {
                tools.status = "Road selected: change lanes, direction or curve in the panel.".into();
            }
            // [road-inspect] The piece clicked, for the road card
            // beside the upgrade panel (none for a road only planned).
            let piece = ground.filter(|_| tools.selected_gesture.is_some()).and_then(|p| crate::road_select::pick(&sim.world, p, tools.depth));
            match piece {
                Some(sid) => *selection = Selection::Road(sid),
                None if matches!(*selection, Selection::Road(_)) => *selection = Selection::None,
                None => {}
            }
        }
    } else if shift_click {
        if let (Some(p), Some((id, seg))) = (ground, ground.and_then(|p| nearest_gesture(plan, p))) {
            if let Some(mut g) = plan.gestures.get(&id).cloned() {
                let at = (seg + 1).min(g.points().len());
                if g.points_mut().is_some() {
                    g.insert_point(at, p);
                    sim.world.set_gesture(id, g, true);
                    tools.status = "Control point inserted".into();
                }
            }
        }
    }
    if map_key(KeyCode::Delete) {
        if let Some((id, idx)) = tools.hover_point {
            if let Some(mut g) = plan.gestures.get(&id).cloned() {
                if g.points().len() > g.min_points() {
                    g.remove_point(idx);
                    sim.world.set_gesture(id, g, true);
                    tools.status = "Control point removed".into();
                } else {
                    sim.world.remove_gesture(id);
                    tools.status = "Gesture removed".into();
                }
            }
        }
    }
}

/// Every control point (the hovered or dragged one bright), the hovered
/// and selected gestures, and the selected road's lanes.
pub(super) fn preview(gizmos: &mut Gizmos, p: &Preview) {
    let Preview { tools, sim, plan, .. } = *p;
    let lift = |q: SVec2| super::lift(sim, q);
    let (cyan, white) = (super::CYAN, super::WHITE);
    for (id, g) in &plan.gestures {
        for (i, &p) in g.points().iter().enumerate() {
            let hovered = tools.hover_point == Some((*id, i)) || tools.drag == Some((*id, i));
            gizmos.circle(flat_at(lift(p)), if hovered { 5.0 } else { 2.5 }, if hovered { white } else { cyan });
        }
    }
    for (id, colour) in [(tools.hover_gesture, Color::linear_rgb(0.0, 1.2, 1.5)), (tools.selected_gesture, Color::linear_rgb(2.5, 1.6, 0.4))] {
        let Some(id) = id else { continue };
        if let Some(g) = plan.gestures.get(&id) {
            let mut pts: Vec<Vec3> = g.road_polyline().iter().map(|&p| lift(p)).collect();
            if !g.is_road() && pts.len() >= 3 {
                if let Some(&first) = pts.first() { pts.push(first); }
            }
            gizmos.linestrip(pts, colour);
        }
    }
    // [clarity] The selected road's lanes as big arrows, one per
    // lane in its direction of travel, so "Flip direction" is no
    // guesswork.
    if let Some(g) = tools.selected_gesture.and_then(|id| plan.gestures.get(&id)) {
        if let Gesture::Road { lanes_fwd, lanes_bwd, kind: RoadKind::Street, .. } = g {
            lane_arrows(gizmos, &g.road_polyline(), *lanes_fwd, *lanes_bwd, &lift);
        }
    }
}

/// [clarity] Big arrows along a road, one per lane, pointing the way its
/// traffic runs: lanes forward (along the drawing) on the right of the
/// line, lanes back on the left (right-hand traffic); a one-way road's
/// lanes centred on it. Repeated every 45 m or so, at least once.
fn lane_arrows(gizmos: &mut Gizmos, line: &[SVec2], fwd: u8, bwd: u8, lift: &dyn Fn(SVec2) -> Vec3) {
    if line.len() < 2 || fwd + bwd == 0 {
        return;
    }
    let lane_w = nd_sim::roads::LANE_WIDTH;
    let total: f32 = line.windows(2).map(|w| w[0].distance(w[1])).sum();
    let n = ((total / 45.0).floor() as usize).max(1);
    let fwd_col = Color::linear_rgb(0.4, 3.2, 1.0);
    let bwd_col = Color::linear_rgb(3.2, 1.2, 0.3);
    for k in 0..n {
        let s = total * (k as f32 + 0.5) / n as f32;
        let (p, d) = nd_sim::rail::point_along(line, s);
        let d = d.normalize_or_zero();
        if d == SVec2::ZERO {
            continue;
        }
        let right = SVec2::new(-d.y, d.x);
        let mut lanes: Vec<(f32, f32, Color)> = Vec::new();
        if bwd == 0 || fwd == 0 {
            let (m, sign, col) = if bwd == 0 { (fwd, 1.0, fwd_col) } else { (bwd, -1.0, bwd_col) };
            for i in 0..m {
                lanes.push(((i as f32 - (m as f32 - 1.0) * 0.5) * lane_w, sign, col));
            }
        } else {
            for i in 0..fwd {
                lanes.push(((i as f32 + 0.5) * lane_w, 1.0, fwd_col));
            }
            for i in 0..bwd {
                lanes.push((-(i as f32 + 0.5) * lane_w, -1.0, bwd_col));
            }
        }
        for (off, sign, col) in lanes {
            let c = p + right * off;
            let dir = d * sign;
            let tail = c - dir * 5.0;
            let tip = c + dir * 5.0;
            let side = SVec2::new(-dir.y, dir.x);
            let up = Vec3::Y * 1.4;
            for y in [up, up + Vec3::Y * 0.25] {
                gizmos.line(lift(tail) + y, lift(tip) + y, col);
                gizmos.line(lift(tip) + y, lift(tip - dir * 2.6 + side * 1.4) + y, col);
                gizmos.line(lift(tip) + y, lift(tip - dir * 2.6 - side * 1.4) + y, col);
            }
        }
    }
}

/// [fix/review-app] One frame of a control-point drag of gesture `id`
/// from `before` to `after`. The first frame that moves the point records
/// a new undo step, so the shape before the drag stays one Ctrl+Z away;
/// every later frame, the release included, amends that step. (Amending
/// from the first frame overwrote the step that drew the gesture: undo
/// then kept the moved shape, and a second undo deleted it.)
pub fn drag_frame(world: &mut nd_sim::World, id: GestureId, before: &nd_sim::Gesture, after: nd_sim::Gesture, moved: &mut bool) {
    if *moved {
        world.set_gesture(id, after, false);
    } else if after != *before {
        world.set_gesture(id, after, true);
        *moved = true;
    }
}

#[cfg(test)]
mod pick_tests {
    use super::*;

    fn road(points: Vec<SVec2>, f: u8, b: u8) -> Gesture {
        Gesture::Road { points, lanes_fwd: f, lanes_bwd: b, curved: false, kind: RoadKind::Street, elevation: Vec::new() }
    }

    /// A ramp drawn onto a highway's start shares its control point. A click
    /// there, or on the ramp beside it, picks the ramp: picking the older
    /// highway turned the whole of it into a ramp when the player then set
    /// the Ramp preset in the upgrade panel.
    #[test]
    fn a_click_at_a_joint_picks_the_road_attached_there() {
        let mut plan = Plan::default();
        let start = SVec2::new(-400.0, 0.0);
        plan.gestures.insert(GestureId(1), road(vec![start, SVec2::new(-150.0, 10.0), SVec2::new(100.0, 0.0)], 4, 0));
        plan.gestures.insert(GestureId(2), road(vec![SVec2::new(-600.0, 120.0), SVec2::new(-480.0, 40.0), start], 1, 0));
        // Exactly on the joint: the newer road.
        assert_eq!(nearest_control_point(&plan, start), Some((GestureId(2), 2)));
        assert_eq!(nearest_gesture(&plan, start).map(|g| g.0), Some(GestureId(2)));
        // A little up the ramp: the ramp.
        let on_ramp = start + (SVec2::new(-480.0, 40.0) - start).normalize() * 3.0;
        assert_eq!(nearest_control_point(&plan, on_ramp), Some((GestureId(2), 2)));
        assert_eq!(nearest_gesture(&plan, on_ramp).map(|g| g.0), Some(GestureId(2)));
        // A little along the highway: the highway.
        let on_highway = start + (SVec2::new(-150.0, 10.0) - start).normalize() * 3.0;
        assert_eq!(nearest_control_point(&plan, on_highway), Some((GestureId(1), 0)));
        assert_eq!(nearest_gesture(&plan, on_highway).map(|g| g.0), Some(GestureId(1)));
    }
}

#[cfg(test)]
mod drag_tests {
    use super::*;
    use nd_sim::World;

    fn road(x: f32) -> Gesture {
        Gesture::Road { points: vec![SVec2::new(x, -300.0), SVec2::new(x, 300.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() }
    }

    /// Run a drag of `id`'s first point to x = 300, 320, 340 (release on
    /// the last), the way the Edit tool calls `drag_frame`.
    fn drag(w: &mut World, id: GestureId) {
        let mut moved = false;
        let mut current = w.gesture(id).unwrap();
        // A frame that does not move it yet records nothing.
        drag_frame(w, id, &current.clone(), current.clone(), &mut moved);
        for x in [300.0, 320.0, 340.0] {
            let mut g = current.clone();
            g.move_point(0, SVec2::new(x, -300.0));
            drag_frame(w, id, &current, g.clone(), &mut moved);
            current = g;
        }
    }

    /// [fix/review-app] Draw a road, drag a control point, undo: the road is
    /// back where it was drawn (one undo), and a second undo removes it.
    #[test]
    fn undo_after_drag_restores_the_drawn_shape() {
        let mut w = World::new(nd_sim::WorldConfig { landscape: nd_sim::LandscapeParams::flat(), ..Default::default() });
        let id = w.add_gesture(road(200.0));
        drag(&mut w, id);
        assert_eq!(w.gesture(id).unwrap().points()[0], SVec2::new(340.0, -300.0));
        assert!(w.undo());
        assert_eq!(w.gesture(id), Some(road(200.0)), "one undo puts the drawn shape back");
        assert!(w.redo());
        assert_eq!(w.gesture(id).unwrap().points()[0], SVec2::new(340.0, -300.0), "redo moves it again");
        assert!(w.undo());
        assert!(w.undo());
        assert_eq!(w.gesture(id), None, "the second undo removes the road");
        // Two drags in a row are two undo steps.
        assert!(w.redo());
        drag(&mut w, id);
        drag(&mut w, id);
        assert!(w.undo());
        assert!(w.gesture(id).is_some());
        assert!(w.undo());
        assert_eq!(w.gesture(id), Some(road(200.0)));
    }
}
