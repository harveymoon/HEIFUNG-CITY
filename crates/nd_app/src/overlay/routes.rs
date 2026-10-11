//! [overlay] Lines over the map that are not the selection: the bus lines
//! (every one with the Buses tool or the toggle, else the one picked) and
//! the metro lines with the toggle, and the rage marks over drivers who
//! have lost their temper.

use bevy::prelude::*;
use nd_sim::Vec2 as SVec2;

use super::shapes::flat_at;
use crate::camera::{Cursor, OrbitCamera};
use crate::sim::Sim;
use crate::tools::{Tool, ToolState, TransitMode};

/// [rage] How close the camera must be for angry drivers' "!" marks.
const RAGE_MARK_RANGE: f32 = 260.0;

/// [rage] Drivers who have lost their temper: a small red "!" pulsing
/// over the car, quicker the angrier they are. Only when the camera is
/// close (`cam`, or any distance in the walk view); from afar it would
/// just be noise.
pub fn rage_marks(gizmos: &mut Gizmos, sim: &Sim, t: f32, cam: Option<&OrbitCamera>, walk: bool) {
    let Some(cam) = cam.filter(|c| walk || c.distance < RAGE_MARK_RANGE) else { return };
    let f = sim.frac();
    let near = SVec2::new(cam.target.x, cam.target.z);
    for (id, car) in sim.world.traffic.cars.iter().filter(|(_, c)| c.drive.anger >= 0.5) {
        let (p, _) = car.pose_at(f);
        if p.distance(near) > RAGE_MARK_RANGE {
            continue;
        }
        let phase = (slotmap::Key::data(&id).as_ffi() % 97) as f32 * 0.13;
        let pulse = 0.55 + 0.45 * (t * (3.0 + 4.0 * car.drive.anger) + phase).sin();
        let col = Color::linear_rgb(4.0 * pulse, 0.25 * pulse, 0.15 * pulse);
        let base = Vec3::new(p.x, sim.world.car_height(car) + 2.2, p.y);
        // The stroke (three lines so it reads thicker) and the dot.
        for dx in [-0.06, 0.0, 0.06] {
            gizmos.line(base + Vec3::new(dx, 0.55, 0.0), base + Vec3::new(dx, 1.5, 0.0), col);
        }
        gizmos.sphere(Isometry3d::from_translation(base + Vec3::Y * 0.22), 0.11, col);
    }
}

/// Bus lines: the lane path of every leg in the line's colour, stops as
/// rings, broken legs red. With the Buses tool or the toggle every line
/// shows; otherwise only a selected one (picked in City Data's Services,
/// Transit, the Buses list or by clicking one of its buses), drawn bright
/// with marker posts at its stops and a pulsing ring over each of its
/// buses. [metro-ux] Not while drawing a metro line: the Metro layer is
/// about the metro. [lines] While a line is edited the others are dimmed,
/// the metro is left out, and the pointer shows where a click would put a
/// stop (the leg it would split, or the slot picked in the Lines window).
pub fn bus_lines(gizmos: &mut Gizmos, tools: &ToolState, cursor: &Cursor, sim: &Sim, t: f32) {
    let cyan = Color::linear_rgb(1.4, 1.6, 1.8);
    let red = Color::linear_rgb(3.0, 0.2, 0.4);
    let white = Color::linear_rgb(3.0, 3.0, 3.0);
    let lift = |p: SVec2| Vec3::new(p.x, 0.8 + sim.world.surface_height(p), p.y);
    let metro_mode = tools.tool == Tool::Transit && tools.transit_mode == TransitMode::Metro;
    // [lines] In metro mode no bus lines at all (and no metro in bus mode).
    if metro_mode {
        return;
    }
    let bus_mode = tools.tool == Tool::Transit && tools.transit_mode == TransitMode::Route;
    let editing = tools.line_edit.filter(|e| sim.world.transit.routes.contains_key(e.route));
    let all_lines = bus_mode || tools.show_routes || editing.is_some();
    let pulse = 0.5 + 0.5 * (t * 4.0).sin();
    let road_lift = |p: SVec2| Vec3::new(p.x, 1.2 + sim.world.surface_height(p), p.y);
    if all_lines || tools.route_sel.is_some() {
        let palette = [[0.2, 1.4, 2.2], [2.4, 0.5, 0.3], [0.5, 2.2, 0.5], [2.2, 1.8, 0.3], [1.8, 0.4, 2.0], [0.3, 1.0, 2.4], [2.2, 1.0, 0.6], [1.2, 1.2, 1.2]];
        for (id, route) in &sim.world.transit.routes {
            let sel = editing.map_or(tools.route_sel == Some(id), |e| e.route == id);
            if !all_lines && !sel {
                continue;
            }
            let c = palette[route.color as usize % palette.len()];
            let k = if sel { 1.6 } else if editing.is_some() { 0.22 } else { 0.7 };
            let col = Color::linear_rgb(c[0] * k, c[1] * k, c[2] * k);
            for (i, leg) in route.legs.iter().enumerate() {
                let pts: Vec<Vec3> = leg.iter().map(|&p| road_lift(p)).collect();
                if pts.len() >= 2 {
                    let leg_col = if route.broken.contains(&i) && (sel || editing.is_none()) { red } else { col };
                    if sel {
                        // Gizmo lines are a pixel wide: a second pass just
                        // above makes the selected line read as a band.
                        gizmos.linestrip(pts.iter().map(|p| *p + Vec3::Y * 0.6), leg_col);
                    }
                    gizmos.linestrip(pts, leg_col);
                }
            }
            for (i, s) in route.stops.iter().enumerate() {
                let hovered = tools.hover_stop == Some((id, i));
                let at = road_lift(s.pos);
                gizmos.circle(flat_at(at), if hovered { 4.0 } else if sel { 3.2 } else { 2.4 }, if hovered { white } else { col });
                if sel {
                    gizmos.line(at, at + Vec3::Y * 9.0, col);
                    gizmos.circle(flat_at(at + Vec3::Y * 9.0), 1.0, col);
                }
                // [lines] The stop picked in the Lines window: a pulsing
                // white ring and a post three times as tall.
                if tools.stop_sel == Some((id, i)) {
                    gizmos.circle(flat_at(at), 5.0 + 2.0 * pulse, white);
                    gizmos.line(at, at + Vec3::Y * 28.0, white);
                    gizmos.circle(flat_at(at + Vec3::Y * 28.0), 2.0, white);
                }
            }
            if sel {
                let f = sim.frac();
                let bright = Color::linear_rgb(c[0] * (1.6 + pulse), c[1] * (1.6 + pulse), c[2] * (1.6 + pulse));
                for car in sim.world.traffic.cars.values().filter(|car| car.bus.as_ref().is_some_and(|b| b.route == id)) {
                    let p = car.prev_pos.lerp(car.pos, f);
                    let at = Vec3::new(p.x, sim.world.car_height(car) + 0.3, p.y);
                    gizmos.circle(flat_at(at), 7.0 + 1.5 * pulse, bright);
                    gizmos.line(at + Vec3::Y * 4.0, at + Vec3::Y * 16.0, bright);
                }
            }
        }
        // [subway] Metro lines over the map with the toggle; [lines] not
        // with the Buses tool's Lines mode in hand.
        if tools.show_routes && !bus_mode {
            for l in sim.world.subway.lines.values() {
                let c = nd_procgen::metro::line_colour(l.color);
                let col = Color::linear_rgb(c[0], c[1], c[2]);
                gizmos.linestrip(l.path.iter().map(|&p| lift(p) + Vec3::Y * 0.5), col);
                for &s in &l.stations {
                    if let Some(p) = sim.world.metro_platform(s) {
                        gizmos.circle(flat_at(lift(p) + Vec3::Y * 0.5), 6.0, col);
                    }
                }
            }
        }
        if bus_mode && editing.is_none() {
            if let Some(c) = cursor.ground {
                let on_street = sim.world.roads.nearest_street(c).is_some_and(|(_, _, q)| q.distance(c) <= nd_sim::transit::STOP_SNAP);
                if on_street && tools.hover_stop.is_none() {
                    gizmos.circle(flat_at(lift(c)), 2.0, cyan);
                }
            }
        }
    }
    if let Some(e) = editing {
        edit_preview(gizmos, tools, cursor, sim, &e, pulse);
    }
}

/// [lines] Editing a line: the slot picked in the Lines window (its leg
/// pulsing white), and under the pointer where a click would put a stop
/// (green leads from the stops either side and a "+"), the stop that would
/// be dragged, or the drag under way.
fn edit_preview(gizmos: &mut Gizmos, tools: &ToolState, cursor: &Cursor, sim: &Sim, e: &crate::tools::line_edit::LineEdit, pulse: f32) {
    use crate::tools::line_edit::{hit_at, preview, Hit};
    let w = &sim.world;
    let Some(route) = w.transit.routes.get(e.route) else { return };
    let white = Color::linear_rgb(3.0, 3.0, 3.0);
    let green = Color::linear_rgb(0.6, 3.0, 1.0);
    let lift = |p: SVec2| Vec3::new(p.x, 1.9 + w.surface_height(p), p.y);
    let n = route.stops.len();
    if let Some(s) = e.slot.filter(|_| n >= 2) {
        let leg = (s + n - 1) % n;
        if let Some(pts) = route.legs.get(leg) {
            let k = 3.0 * pulse + 0.5;
            gizmos.linestrip(pts.iter().map(|&p| lift(p)), Color::linear_rgb(k, k, k));
        }
    }
    let Some(c) = cursor.ground.or(tools.cursor_pin) else { return };
    if let Some((r, i)) = tools.stop_drag {
        if let Some(s) = w.transit.routes.get(r).and_then(|x| x.stops.get(i)) {
            gizmos.line(lift(s.pos), lift(c), white);
            gizmos.circle(flat_at(lift(c)), 3.2, white);
        }
        return;
    }
    let hit = hit_at(w, e.route, c);
    if let Hit::Own(i) = hit {
        // Grab: a ring and four ticks.
        let at = lift(route.stops[i].pos);
        gizmos.circle(flat_at(at), 5.0, white);
        for d in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
            gizmos.line(at + d * 5.5, at + d * 7.5, white);
        }
        return;
    }
    let Some((at, idx)) = preview(w, e, hit, tools.line_shift) else { return };
    let p = lift(at);
    if n >= 1 {
        gizmos.line(lift(route.stops[(idx + n - 1) % n].pos), p, green);
        if n >= 2 {
            gizmos.line(p, lift(route.stops[idx % n].pos), green);
        }
    }
    // The "+".
    gizmos.circle(flat_at(p), 3.6, green);
    gizmos.line(p + Vec3::X * 2.4, p - Vec3::X * 2.4, green);
    gizmos.line(p + Vec3::Z * 2.4, p - Vec3::Z * 2.4, green);
}
