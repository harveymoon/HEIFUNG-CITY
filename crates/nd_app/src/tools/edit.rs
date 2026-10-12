//! The Edit tool: pick a road to upgrade, drag, insert and delete control
//! points, offer the Intersection editor on a hovered junction; its preview (every
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
            Gesture::Roundabout { center, radius, lanes, .. } => {
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
///
/// [fix/roads-tools] A junction is one joint: pressing on it drags every
/// road meeting there together (a road running through gets a point there
/// as the drag starts). A road picked first (a click on its line) has its
/// own points dragged on their own instead: that is how a road is taken
/// off a junction deliberately.
pub(super) fn input(f: &Frame, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection) {
    let Frame { click, ground, shift_click, buttons, plan, .. } = *f;
    let map_key = |k: KeyCode| f.map_key(k);
    tools.hover_point = ground.and_then(|p| nearest_control_point(plan, p));
    tools.hover_gesture = ground.and_then(|p| nearest_gesture(plan, p)).map(|(id, _)| id);
    tools.hover_joint = if tools.drag.is_some() || tools.joint_drag.is_some() { None } else { ground.and_then(|p| sim.world.joint_at(plan, p, POINT_PICK_RADIUS)) };
    // [pt-ux] A junction under the pointer (or the remote API's pin) is
    // offered with an "Edit intersection" button; a click there edits the
    // roads as anywhere else.
    tools.junction_hover = if tools.drag.is_some() || tools.joint_drag.is_some() { None } else { tools.cursor_pin.or(ground).and_then(|p| crate::ui::junction::target_at(&sim.world, p)) };

    if let Some(mut jd) = tools.joint_drag.take() {
        let release = !buttons.pressed(MouseButton::Left);
        if let Some(p) = ground {
            joint_drag_frame(&mut sim.world, &mut jd, p);
        }
        if release {
            tools.status = if jd.moved { format!("Junction moved: {} road{} with it. Ctrl+Z undoes.", jd.roads(), if jd.roads() == 1 { "" } else { "s" }) } else { String::new() };
        } else {
            tools.joint_drag = Some(jd);
        }
    } else if let Some((id, idx)) = tools.drag {
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
        // [pt-ux] The Intersection editor opens from the button beside a
        // hovered junction, not from a click on it.
        match press(tools.hover_joint.as_ref(), tools.selected_gesture, plan) {
            Press::Joint(jd) => {
                tools.status = format!("Moving the junction: {} roads move with it. To take one road off it, click that road first, then drag its end.", jd.roads());
                tools.joint_drag = Some(jd);
                tools.selected_gesture = None;
            }
            Press::Own(jd) => {
                tools.status = "Moving the picked road's own point: it comes off the junction.".into();
                tools.joint_drag = Some(jd);
            }
            Press::Point(hp) => {
                tools.drag = Some(hp);
                tools.drag_moved = false;
                tools.selected_gesture = Some(hp.0);
            }
            Press::Nothing => {
                tools.selected_gesture = tools.hover_gesture;
                if tools.selected_gesture.is_some() {
                    tools.status = "Road picked: change lanes, direction or curve in the panel; drag its own points (even at a junction) to reshape or detach it.".into();
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

/// [fix/roads-tools] What a press of the left button in the Edit tool
/// grabs.
pub(super) enum Press {
    /// A junction: every road meeting there.
    Joint(super::JointDrag),
    /// The picked road's own point at a junction (it comes off it).
    Own(super::JointDrag),
    /// A lone control point.
    Point((GestureId, usize)),
    /// No point: a click picks the road under it.
    Nothing,
}

/// [fix/roads-tools] What pressing at the junction (or point) `joint`
/// grabs, with `picked` the road picked before.
pub(super) fn press(joint: Option<&nd_sim::Joint>, picked: Option<GestureId>, plan: &Plan) -> Press {
    let Some(j) = joint else { return Press::Nothing };
    if j.is_junction() {
        if let Some(own) = picked.filter(|g| j.gestures().contains(g)) {
            let mine = nd_sim::Joint {
                at: j.at,
                node: j.node,
                points: j.points.iter().copied().filter(|m| m.0 == own).take(1).collect(),
                through: if j.points.iter().any(|m| m.0 == own) { Vec::new() } else { j.through.iter().copied().filter(|m| m.0 == own).collect() },
            };
            return Press::Own(super::JointDrag::of(&mine, plan));
        }
        return Press::Joint(super::JointDrag::of(j, plan));
    }
    match j.points.first() {
        Some(&p) => Press::Point(p),
        None => Press::Nothing,
    }
}

/// [fix/roads-tools] One frame of a junction drag to `p`: the first frame
/// that moves it records a new undo step, later ones amend it.
pub fn joint_drag_frame(world: &mut nd_sim::World, jd: &mut super::JointDrag, p: SVec2) {
    let changes = jd.at(p);
    if jd.moved {
        world.set_gestures(changes, false);
    } else if changes.iter().any(|(id, g)| jd.base.get(id).is_some_and(|b| b != g)) {
        world.set_gestures(changes, true);
        jd.moved = true;
    }
}

/// Every control point (the hovered or dragged one bright), the hovered
/// and selected gestures, and the selected road's lanes.
pub(super) fn preview(gizmos: &mut Gizmos, p: &Preview) {
    let Preview { tools, sim, plan, .. } = *p;
    let lift = |q: SVec2| super::lift(sim, q);
    let (cyan, white) = (super::CYAN, super::WHITE);
    // [fix/roads-tools] One dot per junction: control points that coincide
    // (roads meeting there) and built junctions with no point of their own
    // yet (a road running through) are drawn once, as a joint. The picked
    // road's own points are drawn orange: those drag on their own.
    let gold = Color::linear_rgb(2.6, 1.7, 0.3);
    let orange = Color::linear_rgb(3.0, 0.9, 0.2);
    let key = |p: SVec2| ((p.x * 2.0).round() as i64, (p.y * 2.0).round() as i64);
    let mut dots: std::collections::HashMap<(i64, i64), (SVec2, usize)> = std::collections::HashMap::new();
    for g in plan.gestures.values() {
        for &p in g.points() {
            dots.entry(key(p)).or_insert((p, 0)).1 += 1;
        }
    }
    let roads = &sim.world.roads;
    for n in &roads.nodes {
        let live = n.segments.iter().filter(|&&s| roads.try_segment(s).is_some()).count();
        if live >= 3 && !dots.contains_key(&key(n.pos)) && !n.segments.iter().any(|&s| roads.try_segment(s).is_some_and(|s| s.ring)) {
            dots.insert(key(n.pos), (n.pos, 2));
        }
    }
    for (p, count) in dots.values() {
        if *count >= 2 {
            let c = flat_at(lift(*p));
            gizmos.circle(c, 3.2, gold);
            gizmos.circle(c, 1.4, gold);
        } else {
            gizmos.circle(flat_at(lift(*p)), 2.5, cyan);
        }
    }
    if let Some(g) = tools.selected_gesture.and_then(|id| plan.gestures.get(&id)) {
        for &p in g.points() {
            gizmos.circle(flat_at(lift(p) + Vec3::Y * 0.2), 2.0, orange);
        }
    }
    // The point or junction under the pointer (or being dragged), and the
    // roads that would move with it.
    if let Some((id, i)) = tools.drag {
        if let Some(&p) = plan.gestures.get(&id).and_then(|g| g.points().get(i)) {
            gizmos.circle(flat_at(lift(p)), 5.0, white);
        }
    } else if let Some(j) = tools.hover_joint.as_ref() {
        let own = tools.selected_gesture.filter(|g| j.is_junction() && j.gestures().contains(g));
        let c = flat_at(lift(j.at));
        gizmos.circle(c, 5.0, white);
        if j.is_junction() && own.is_none() {
            gizmos.circle(c, 6.2, white);
            for id in j.gestures() {
                if let Some(g) = plan.gestures.get(&id) {
                    gizmos.linestrip(g.road_polyline().iter().map(|&p| lift(p) + Vec3::Y * 0.3).collect::<Vec<_>>(), Color::linear_rgb(0.0, 1.2, 1.5));
                }
            }
        }
    }
    if let Some(jd) = tools.joint_drag.as_ref() {
        for (id, i) in &jd.moving {
            if let Some(&p) = plan.gestures.get(id).and_then(|g| g.points().get(*i)) {
                gizmos.circle(flat_at(lift(p)), 5.0, white);
            }
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
    // [pt-ux] The junction offered for the Intersection editor: a ring.
    if let Some(at) = tools.junction_hover.filter(|_| tools.junction_editor.is_none()) {
        let r = sim.world.junction_at(at, 2.0).and_then(|k| sim.world.traffic.net.intersections.get(k)).map_or(8.0, |i| i.radius + 3.0);
        for k in 0..2 {
            gizmos.circle(flat_at(lift(at) + Vec3::Y * (0.6 + 0.3 * k as f32)), r, Color::linear_rgb(0.4, 2.2, 3.0));
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
        Gesture::Road { points, lanes_fwd: f, lanes_bwd: b, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None }
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
        Gesture::Road { points: vec![SVec2::new(x, -300.0), SVec2::new(x, 300.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None }
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

/// [fix/roads-tools] The remote API's `tool {edit_drag}`: press the left
/// button at `from` in the Edit tool (with the road picked as it is) and
/// drag to `to`, as the mouse would. What was grabbed ("junction", "own"
/// for the picked road's own point, "point", or "nothing") and how many
/// roads moved.
pub(crate) fn remote_drag(world: &mut nd_sim::World, tools: &mut ToolState, from: SVec2, to: SVec2) -> (&'static str, usize) {
    let plan = world.effective_plan().clone();
    let joint = world.joint_at(&plan, from, POINT_PICK_RADIUS);
    match press(joint.as_ref(), tools.selected_gesture, &plan) {
        Press::Joint(mut jd) | Press::Own(mut jd) => {
            let own = tools.selected_gesture.is_some_and(|g| jd.base.contains_key(&g)) && jd.base.len() == 1;
            joint_drag_frame(world, &mut jd, to);
            if !own {
                tools.selected_gesture = None;
            }
            (if own { "own" } else { "junction" }, jd.roads())
        }
        Press::Point((id, i)) => {
            let Some(before) = plan.gestures.get(&id).cloned() else { return ("nothing", 0) };
            let mut g = before.clone();
            g.move_point(i, to);
            let mut moved = false;
            drag_frame(world, id, &before, g, &mut moved);
            tools.selected_gesture = Some(id);
            ("point", 1)
        }
        Press::Nothing => ("nothing", 0),
    }
}
