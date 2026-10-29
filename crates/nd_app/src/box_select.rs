//! [K34] A shipping container selected: where it is (in a depot's stacks
//! or on a train as drawn, else on its lorry, else at the building that
//! holds it), its name, its outline, and picking one with a click.

use crate::sim::Sim;
use bevy::prelude::*;
use nd_sim::containers::{BoxId, Where};

/// "KOLU 482913 7 (Kestrel Ocean Lines)".
pub fn title(w: &nd_sim::World, id: BoxId) -> String {
    w.containers.boxes.get(id).map_or_else(|| "a container".to_string(), |r| format!("{} ({})", r.number(), nd_sim::containers::LINES[r.line as usize % 10].name))
}

/// Where the box is drawn: its floor centre and heading (radians in the
/// sim's x-y plane), and its length.
pub fn pose(sim: &Sim, id: BoxId) -> Option<(Vec3, f32, f32)> {
    let w = &sim.world;
    let r = w.containers.boxes.get(id)?;
    let len = if r.forty { nd_procgen::CONTAINER_40 } else { nd_procgen::CONTAINER_20 };
    if let Some((_, at, rot, _)) = sim.box_spots.iter().find(|s| s.0 == id) {
        let d = *rot * Vec3::X;
        return Some((*at, d.z.atan2(d.x), len));
    }
    match r.at {
        Where::Truck(c) => w.traffic.cars.get(c).map(|car| {
            let (p, h) = car.pose_at(sim.frac());
            (Vec3::new(p.x, w.car_height_at(car, p) + 1.15, p.y), h, len)
        }),
        Where::Stored(b) | Where::Site(b) | Where::Home(b) | Where::Scrap(b) => w.building_lot(b).map(|l| (Vec3::new(l.center.x, w.surface_height(l.center), l.center.y), 0.0, len)),
        Where::Train => None,
    }
}

/// The selected box: an outline round it.
pub fn draw(gizmos: &mut Gizmos, sim: &Sim, id: BoxId) {
    let Some((at, h, len)) = pose(sim, id) else { return };
    let rot = Quat::from_rotation_y(-h);
    let size = Vec3::new(len + 0.3, nd_procgen::CONTAINER_H + 0.3, nd_procgen::CONTAINER_W + 0.3);
    let c = at + Vec3::Y * (nd_procgen::CONTAINER_H * 0.5);
    gizmos.cube(Transform::from_translation(c).with_rotation(rot).with_scale(size), Color::srgb(1.0, 0.78, 0.2));
}

/// The nearest drawn box a ray from `o` along `d` meets, and how far.
pub fn pick_ray(sim: &Sim, o: Vec3, d: Vec3) -> Option<(f32, BoxId)> {
    let mut best: Option<(f32, BoxId)> = None;
    for &(id, at, rot, forty) in &sim.box_spots {
        let len = if forty { nd_procgen::CONTAINER_40 } else { nd_procgen::CONTAINER_20 };
        let half = Vec3::new(len * 0.5, nd_procgen::CONTAINER_H * 0.5, nd_procgen::CONTAINER_W * 0.5);
        let inv = rot.inverse();
        let lo = inv * (o - (at + Vec3::Y * half.y));
        let ld = inv * d;
        let (mut t0, mut t1) = (0.0f32, f32::INFINITY);
        for k in 0..3 {
            let (oo, dd, hh) = (lo[k], ld[k], half[k]);
            if dd.abs() < 1e-6 {
                if oo.abs() > hh {
                    t0 = f32::INFINITY;
                }
                continue;
            }
            let (a, b) = ((-hh - oo) / dd, (hh - oo) / dd);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
        if t0 <= t1 && t0.is_finite() && best.is_none_or(|(bt, _)| t0 < bt) {
            best = Some((t0, id));
        }
    }
    best
}
