//! [r14-foodtrucks] Food trucks (`world.food_trucks`) as things to select:
//! the click on one at the kerb, the outline, its title, where it is.

use bevy::prelude::*;
use nd_sim::food_trucks::{TruckId, TruckPlace};
use nd_sim::{Vec2 as SVec2, World};

use crate::overlay::shapes::{vehicle_box, GOLD};

/// Drawn length and width: a step van or a three-wheeler cart.
pub fn size(w: &World, id: TruckId) -> (f32, f32) {
    match w.food_trucks.trucks.get(id) {
        Some(t) if t.kind.cart() => (3.5, 1.6),
        _ => (6.2, 2.2),
    }
}

/// The truck standing at the kerb under `p` (the awning and stools too).
pub fn pick(w: &World, p: SVec2) -> Option<TruckId> {
    w.parked_food_trucks()
        .filter_map(|(id, at, heading, _, _, _)| {
            let (len, wid) = size(w, id);
            let fwd = SVec2::new(heading.cos(), heading.sin());
            let rel = p - at;
            let (a, b) = (rel.dot(fwd).abs(), rel.perp_dot(fwd).abs());
            (a < len * 0.5 + 1.0 && b < wid * 0.5 + 1.5).then_some((rel.length(), id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

/// "Tacos Nguyen  ·  taco truck".
pub fn title(w: &World, id: TruckId) -> String {
    w.food_trucks.trucks.get(id).map_or_else(|| "a food truck".into(), |t| format!("{}  ·  {}", t.name, t.kind.label()))
}

/// Where it is and the way it faces: at the kerb, on the road (its car),
/// or at home.
pub fn pose(w: &World, id: TruckId, f: f32) -> Option<(SVec2, f32)> {
    let t = w.food_trucks.trucks.get(id)?;
    match t.place {
        TruckPlace::Pitch { pitch, .. } => Some((pitch.pos, pitch.heading)),
        TruckPlace::Road(car) => w.traffic.cars.get(car).map(|c| {
            let p = if c.prev_pos.distance(c.pos) < 30.0 { c.prev_pos.lerp(c.pos, f) } else { c.pos };
            (p, c.heading)
        }),
        TruckPlace::Base => w.building_lot(t.base).map(|l| (l.center, 0.0)),
    }
}

pub fn draw(gizmos: &mut Gizmos, w: &World, id: TruckId, f: f32) {
    let Some(t) = w.food_trucks.trucks.get(id) else { return };
    if t.place == TruckPlace::Base {
        return;
    }
    let Some((p, h)) = pose(w, id, f) else { return };
    let (len, wid) = size(w, id);
    vehicle_box(gizmos, p, h, len + 0.4, wid + 0.4, w.surface_height(p) + 0.35, GOLD);
}
