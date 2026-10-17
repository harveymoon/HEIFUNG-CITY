//! [feat/pt-ux] Drone taxis and helicopters as things to select: where one
//! is drawn this frame, the click's ray against it, its outline and the
//! line to where it is going, its title and what it is doing, and who is
//! aboard (the rider or patient, and a helicopter's crew from its base).

use bevy::prelude::*;
use nd_sim::air::{Craft, DroneId, Job};
use nd_sim::{BuildingId, CitizenId, Vec2 as SVec2, World};

use crate::overlay::shapes::{lerp_angle, ring, vehicle_box, GOLD};

/// Where drone `id` is drawn at `f` of the tick (Bevy coordinates; the
/// renderer eases a landing onto the drawn deck, a metre or two off this)
/// and its heading.
pub fn pose(w: &World, id: DroneId, f: f32) -> Option<(Vec3, f32)> {
    let d = w.air.drones.get(id)?;
    let p = d.prev_pos.lerp(d.pos, f);
    let alt = d.prev_alt + (d.alt - d.prev_alt) * f;
    Some((Vec3::new(p.x, w.surface_height(p) + alt, p.y), lerp_angle(d.prev_heading, d.heading, f)))
}

/// The nearest aircraft the ray `o + d t` (`d` a unit vector) passes
/// close to, and `t`. The reach grows with the distance so a helicopter far
/// off is still a fair click.
pub fn pick_ray(w: &World, f: f32, o: Vec3, d: Vec3) -> Option<(f32, DroneId)> {
    w.air
        .drones
        .keys()
        .filter_map(|id| {
            let (at, _) = pose(w, id, f)?;
            let t = (at - o).dot(d);
            if t <= 0.0 {
                return None;
            }
            let miss = (o + d * t).distance(at);
            (miss < 5.0 + t * 0.006).then_some((t, id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// "Air Medic 2 · St Mary's" and the like.
pub fn title(w: &World, id: DroneId) -> String {
    let Some(d) = w.air.drones.get(id) else { return "an aircraft".into() };
    let base = w.buildings.get(d.base).map_or("", |b| b.name.as_str());
    format!("{} {}  ·  {base}", d.craft.call_sign(), d.number)
}

/// What it is doing and where it is going.
pub fn doing(w: &World, id: DroneId) -> String {
    let Some(d) = w.air.drones.get(id) else { return String::new() };
    let to = destination(w, id).map(|(name, _)| format!(" to {name}")).unwrap_or_default();
    // [r4] (K28) Put down short of its pad on a flat battery.
    if d.grounded {
        return if d.phase.airborne() { "Battery all but flat  ·  coming down in the street".into() } else { format!("Down in the street, battery flat  ·  charging off the recovery van ({:.0}%)", d.battery * 100.0) };
    }
    // Waiting on the pad for its landing slot at the far end.
    if d.phase == nd_sim::air::Phase::Climbing && d.at.is_some() && d.booked.is_none() && d.dest.is_some() && d.until > w.clock.tick {
        return format!("{}  ·  cleared for take-off once a landing slot{} is free", crate::ui::fmt::capitalise(d.job.label()), to);
    }
    let slot = if d.booked.is_some() && d.phase.airborne() { "  ·  landing slot booked" } else { "" };
    format!("{}  ·  {}{}{slot}", crate::ui::fmt::capitalise(d.job.label()), d.phase.label(), if d.phase.airborne() { to } else { String::new() })
}

/// Where it is flying: a pad building's name (or "the street") and the point.
pub fn destination(w: &World, id: DroneId) -> Option<(String, SVec2)> {
    let d = w.air.drones.get(id)?;
    let name = match d.dest.and_then(|b| w.buildings.get(b)) {
        Some(b) => b.name.clone(),
        None if matches!(d.job, Job::Patrol { .. }) => "a crime scene".into(),
        None if matches!(d.job, Job::Medic { aboard: false, .. }) => "a patient in the street".into(),
        None => "the street".into(),
    };
    Some((name, d.goal))
}

/// The passenger or patient aboard, then (for a helicopter) its crew: the
/// first staff of the base it flies from.
pub fn aboard(w: &World, id: DroneId) -> (Option<CitizenId>, Vec<CitizenId>) {
    let Some(d) = w.air.drones.get(id) else { return (None, Vec::new()) };
    let rider = d.aboard().filter(|c| w.citizens.contains_key(*c));
    let crew = match d.craft {
        // Halo Skyways' drones fly themselves.
        Craft::Taxi => Vec::new(),
        Craft::AirAmbulance | Craft::PoliceHeli => w.buildings.get(d.base).map(|b| b.workers.iter().copied().filter(|c| w.citizens.contains_key(*c)).take(if d.craft == Craft::PoliceHeli { 2 } else { 3 }).collect()).unwrap_or_default(),
    };
    (rider, crew)
}

/// The base and the pad it is bound for (for buttons).
pub fn base_and_dest(w: &World, id: DroneId) -> (Option<BuildingId>, Option<BuildingId>) {
    w.air.drones.get(id).map_or((None, None), |d| (Some(d.base), d.dest.filter(|b| *b != d.base)))
}

/// The selection drawn: a box round it, a ring under it on the ground and
/// a line to where it is going.
pub fn draw(gizmos: &mut Gizmos, w: &World, id: DroneId, f: f32, t: f32) {
    let Some((at, heading)) = pose(w, id, f) else { return };
    let Some(d) = w.air.drones.get(id) else { return };
    let (len, wid) = match d.craft {
        Craft::Taxi => (5.0, 5.0),
        _ => (12.0, 3.5),
    };
    let p = SVec2::new(at.x, at.z);
    vehicle_box(gizmos, p, heading, len, wid, at.y - 0.8, GOLD);
    let ground = w.surface_height(p);
    let pulse = 0.5 + 0.5 * (t * 3.0).sin();
    ring(gizmos, Vec3::new(p.x, ground + 0.3, p.y), 3.0 + 2.0 * pulse, GOLD);
    gizmos.line(Vec3::new(p.x, ground + 0.3, p.y), at, GOLD.with_alpha(0.5));
    if d.phase.airborne() {
        let goal = Vec3::new(d.goal.x, w.surface_height(d.goal) + d.goal_alt + 0.5, d.goal.y);
        let col = Color::linear_rgb(0.5, 2.6, 3.2);
        gizmos.line(at, goal, col);
        ring(gizmos, goal, 4.0, col);
    }
}
