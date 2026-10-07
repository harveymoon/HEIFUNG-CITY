//! [train-inspect] Trains as things to select: picking one with the Inspect
//! tool (a click on any car of it, the metro's only in the Underground
//! view), its name and colours, and the overlay a selected train draws: an
//! outline round every car, the rest of its run along the track as a
//! glowing line in its colour with posts at the stops still to come (the
//! next one tallest, pulsing), and the track already run, faint.

use bevy::prelude::*;
use nd_sim::rail::{self, Train, TrainKind};
use nd_sim::subway::{self as metro, MetroTrainId};
use nd_sim::{TrainId, Vec2 as SVec2, World};

use crate::sim::Sim;
use crate::tools::{route_ends, vehicle_outline, Selection};

/// A headcode for a train, as railway timetables write them: the class
/// digit (1 for an express passenger train, 6 for freight), a district
/// letter and two digits, steady for the train's life (from its id).
pub fn headcode(id: TrainId, kind: TrainKind) -> String {
    let n = slotmap::Key::data(&id).as_ffi();
    let class = match kind {
        TrainKind::Passenger => '1',
        TrainKind::Freight => '6',
    };
    let letter = ['A', 'C', 'E', 'H', 'L', 'M', 'S', 'V'][(n % 8) as usize];
    format!("{class}{letter}{:02}", (n >> 3) % 100)
}

/// A metro run's number on its line: "M" and two digits.
pub fn metro_code(id: MetroTrainId) -> String {
    format!("M{:02}", slotmap::Key::data(&id).as_ffi() % 100)
}

/// The body paint of a surface train (the renderer's car colours).
pub fn body_rgb(palette: &[[f32; 3]], color: u8) -> [f32; 3] {
    palette.get(color as usize % palette.len().max(1)).copied().unwrap_or([0.7, 0.7, 0.75])
}

/// A colour bright enough to glow, with the hue of `rgb`: dark and grey
/// paint (most locomotives) glow an icy white-blue instead of grey.
pub fn glow_rgb(rgb: [f32; 3]) -> [f32; 3] {
    let hi = rgb.iter().copied().fold(0.0f32, f32::max);
    let lo = rgb.iter().copied().fold(f32::INFINITY, f32::min);
    if hi < 0.02 || (hi - lo) / hi < 0.25 {
        return [1.5, 2.2, 3.2];
    }
    let k = 3.0 / hi;
    [rgb[0] * k, rgb[1] * k, rgb[2] * k]
}

/// The glow colour of whatever train `sel` is.
pub fn glow_of(w: &World, sel: Selection, palette: &[[f32; 3]]) -> Option<[f32; 3]> {
    match sel {
        Selection::Train(id) => w.rail.trains.get(id).map(|t| glow_rgb(body_rgb(palette, t.color))),
        Selection::Metro(id) => w.subway.trains.get(id).map(|t| glow_rgb(nd_procgen::metro::line_colour(t.run.color))),
        _ => None,
    }
}

/// Each car of a train as drawn this frame: centre, heading, length.
pub fn car_poses(w: &World, sel: Selection, f: f32) -> Vec<(SVec2, f32, f32)> {
    match sel {
        Selection::Train(id) => {
            let Some(t) = w.rail.trains.get(id) else { return Vec::new() };
            let s = t.prev_s + (t.s - t.prev_s) * f;
            (0..=t.cars as usize)
                .map(|k| {
                    let (p, h) = t.vehicle_pose_at(k, s);
                    (p, h, if k == 0 { rail::LOCO_LEN } else { t.car_len() })
                })
                .collect()
        }
        Selection::Metro(id) => {
            let Some(t) = w.subway.trains.get(id) else { return Vec::new() };
            let s = t.run.prev_s + (t.run.s - t.run.prev_s) * f;
            (0..metro::CARS as usize)
                .map(|k| {
                    let (p, h) = t.car_pose_at(k, s);
                    (p, h, metro::CAR_LEN)
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// How far `p` is from the centreline of a car whose body it falls on, if
/// it falls on one: within the car's length and a little over its width.
fn on_car(p: SVec2, centre: SVec2, heading: f32, len: f32) -> Option<f32> {
    let fwd = SVec2::new(heading.cos(), heading.sin());
    let d = p - centre;
    let along = d.dot(fwd).abs();
    let across = d.perp_dot(fwd).abs();
    (along <= len * 0.5 + 0.8 && across <= 2.6).then_some(across)
}

/// The train with a car under `p`, by its drawn pose, and how far from the
/// car's centreline the click is. Above ground only surface trains count,
/// underground only metro trains (each view shows only its own).
pub fn pick(w: &World, p: SVec2, f: f32, under: bool) -> Option<(f32, Selection)> {
    let sels: Vec<Selection> = if under { w.subway.trains.keys().map(Selection::Metro).collect() } else { w.rail.trains.keys().map(Selection::Train).collect() };
    let mut best: Option<(f32, Selection)> = None;
    for sel in sels {
        for (c, h, len) in car_poses(w, sel, f) {
            if let Some(d) = on_car(p, c, h, len) {
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, sel));
                }
            }
        }
    }
    best
}

/// The railway record of a selected train: a surface train's own, or the
/// run a metro train is making.
pub fn train_of(w: &World, sel: Selection) -> Option<&Train> {
    match sel {
        Selection::Train(id) => w.rail.trains.get(id),
        Selection::Metro(id) => w.subway.trains.get(id).map(|t| &t.run),
        _ => None,
    }
}

/// A selected train's overlay (see the module docs). Metro trains are drawn
/// at tunnel depth in the Underground view and on the surface above their
/// tunnel otherwise. `time` makes the next stop's ring pulse.
pub fn draw(gizmos: &mut Gizmos, sim: &Sim, sel: Selection, palette: &[[f32; 3]], under: bool, time: f32) {
    let w = &sim.world;
    let f = sim.frac();
    let Some(t) = train_of(w, sel) else { return };
    let Some(glow) = glow_of(w, sel, palette) else { return };
    let is_metro = matches!(sel, Selection::Metro(_));
    let below = is_metro && under;
    let ground = |p: SVec2| if below { (if w.relief { w.ground_height(p) } else { 0.0 }) - metro::DEPTH } else { w.surface_height(p) };
    let at = |p: SVec2, y: f32| Vec3::new(p.x, ground(p) + y, p.y);
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let col = |k: f32| Color::linear_rgb(glow[0] * k, glow[1] * k, glow[2] * k);
    let gold = Color::linear_rgb(3.2, 2.4, 0.7);

    // Every car, boxed.
    for (c, h, len) in car_poses(w, sel, f) {
        vehicle_outline(gizmos, c, h, len + 0.8, 3.6, ground(c) + 0.4, gold);
    }

    let s = t.prev_s + (t.s - t.prev_s) * f;
    // The track still to run: a bright core with a halo of lines beside and
    // above it, so it reads as a thick glowing band from any height.
    let ahead = t.route_ahead(s);
    if ahead.len() >= 2 {
        for (side, lift, k) in [(0.0, 1.0, 1.0), (0.0, 1.35, 1.0), (0.0, 1.7, 0.7), (-0.9, 1.0, 0.55), (0.9, 1.0, 0.55), (-1.8, 0.9, 0.3), (1.8, 0.9, 0.3)] {
            let line = if side == 0.0 { ahead.clone() } else { nd_sim::geom::offset_polyline(&ahead, side) };
            gizmos.linestrip(line.iter().map(|&q| at(q, lift)), col(k));
        }
    }
    // The track already run: faint.
    let behind = t.route_behind(s);
    if behind.len() >= 2 {
        gizmos.linestrip(behind.iter().map(|&q| at(q, 0.9)), col(0.16));
    }

    // Stops: still to come as posts and rings (the next tallest, pulsing),
    // those already called at as small faint rings.
    let dwell = if is_metro { metro::DWELL_SECONDS } else { rail::DWELL_SECONDS };
    let calls = t.calls_ahead(dwell, w.config.traffic_seconds_per_tick, |b| w.buildings.contains_key(b));
    let standing = t.dwelling_at();
    for (i, call) in calls.iter().enumerate() {
        let q = rail::point_along(&t.poly, call.s).0;
        let next = i == 0 && standing.is_none();
        let (h, r) = if next { (26.0, 9.0) } else { (14.0, 6.0) };
        gizmos.line(at(q, 0.2), at(q, h), col(if next { 1.2 } else { 0.8 }));
        gizmos.circle(Isometry3d::new(at(q, 1.0), flat), r, col(1.0));
        gizmos.circle(Isometry3d::new(at(q, h), flat), r * 0.35, col(1.0));
        if next {
            let pulse = (time * 3.0).sin() * 0.5 + 0.5;
            gizmos.circle(Isometry3d::new(at(q, 1.2), flat), r + 2.0 + pulse * 4.0, Color::linear_rgb(3.4, 3.2, 2.6));
        }
    }
    if let Some(b) = standing {
        // Standing at a stop: a gold ring and post there.
        if let Some(&(_, sb)) = t.stops.iter().find(|(x, _)| *x == b) {
            let q = rail::point_along(&t.poly, sb).0;
            gizmos.circle(Isometry3d::new(at(q, 1.2), flat), 11.0, gold);
            gizmos.line(at(q, 0.2), at(q, 26.0), gold);
        }
    }
    for &(_, sb) in t.calls_made().iter().filter(|(b, _)| Some(*b) != standing) {
        let q = rail::point_along(&t.poly, sb).0;
        gizmos.circle(Isometry3d::new(at(q, 0.9), flat), 4.0, col(0.25));
    }
    // An "o" where it came in, an "x" where its run ends (on the surface).
    if !below {
        route_ends(gizmos, w, t.poly.first().copied(), t.poly.last().copied());
    }
}
