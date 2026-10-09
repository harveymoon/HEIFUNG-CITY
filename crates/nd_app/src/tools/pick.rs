//! What a click with the Inspect tool picks (`crate::selection::apply_pick`
//! makes it the selection).

use nd_sim::zoning::point_in_polygon;
use nd_sim::{Activity, Vec2 as SVec2};

use super::Depth;
use crate::selection::Selection;
use crate::sim::Sim;

/// What a click at `p` picks: the vehicle drawn under it (tested where it
/// is drawn this frame, not where the sim will have it at the end of the
/// tick), else a person in the open, else a parked car, else a building.
/// The underground view draws only what is below ground, so there a click
/// picks only a vehicle in a tunnel: nothing it could not see.
pub(crate) fn pick(sim: &Sim, p: SVec2, depth: Depth) -> Selection {
    // [fire-scene] An emergency unit's car is the unit (its card follows it
    // from the road onto the scene and back).
    match pick_any(sim, p, depth) {
        Selection::Car(id) => sim.world.services.unit_of_car(id).map_or(Selection::Car(id), Selection::Unit),
        s => s,
    }
}

fn pick_any(sim: &Sim, p: SVec2, depth: Depth) -> Selection {
    let w = &sim.world;
    let f = sim.frac();
    let under = depth == Depth::Underground;
    // Vehicles on the road, by their drawn pose: a click on the body wins.
    let car = w
        .traffic
        .cars
        .iter()
        .map(|(id, car)| (car.pose_at(f).0, id, car))
        .filter(|(at, _, car)| !under || w.car_height_at(car, *at) < crate::render::UNDERGROUND_BELOW)
        .map(|(at, id, car)| (at.distance(p), id, car))
        .filter(|(d, _, car)| *d < if car.bus.is_some() { 8.0 } else { 4.5 })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    // [train-inspect] A train: any car of it, by its drawn pose (the metro's
    // only from underground). A road vehicle right on the spot wins, at a
    // level crossing say.
    let train = crate::train_select::pick(w, p, f, under);
    if let Some((dt, sel)) = train {
        if car.is_none_or(|c| c.0 > dt + 1.5) {
            return sel;
        }
    }
    if under {
        return car.map_or(Selection::None, |(_, id, _)| Selection::Car(id));
    }
    // [fire-scene] A firefighter at work (a click right on them), then a
    // unit standing on scene or on a beat (out of traffic, so not a car).
    if let Some((_, c)) = crate::fire_scene::pick_crew(w, &sim.building_tops, p, f).filter(|(d, _)| *d < 1.0) {
        return Selection::Citizen(c);
    }
    if let Some((du, uid)) = crate::fire_scene::pick_standing(w, p) {
        if car.is_none_or(|c| c.0 > du) {
            return Selection::Unit(uid);
        }
    }
    // People in the open (drivers are picked through their car).
    let person = w
        .citizens
        .iter()
        .filter(|(_, c)| c.is_visible() && !matches!(c.activity, Activity::Driving { .. }))
        .map(|(id, c)| (c.prev_pos.lerp(c.pos, f).distance(p), id))
        .filter(|(d, _)| *d < 6.0)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    // [camera] Pavement delivery robots: small, so a click must be close,
    // and nearer than any car or person it might be standing by.
    let bot = w
        .bots
        .bots
        .iter()
        .map(|(id, b)| ((if b.prev_pos.distance(b.pos) < 30.0 { b.prev_pos.lerp(b.pos, f) } else { b.pos }).distance(p), id))
        .filter(|(d, _)| *d < 2.0)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((db, id)) = bot {
        if car.is_none_or(|c| c.0 > db) && person.is_none_or(|q| q.0 > db) {
            return Selection::Bot(id);
        }
    }
    match (car, person) {
        (Some((dc, id, _)), Some((dp, _))) if dc <= dp + 1.0 => return Selection::Car(id),
        (Some((_, id, _)), None) => return Selection::Car(id),
        (_, Some((_, id))) => return Selection::Citizen(id),
        _ => {}
    }
    // [ferry-routes] A ferry or freighter, by its drawn hull.
    if let Some((_, id)) = crate::ferry_select::pick(w, p, f) {
        return Selection::Ship(id);
    }
    // [toll-inspect] A toll booth (a vehicle in it was picked above).
    if let Some(id) = crate::toll_select::pick(w, p) {
        return Selection::Toll(id);
    }
    // Parked cars at the kerb, in lots and on driveways.
    let parked = w
        .vehicles
        .cars
        .iter()
        .filter_map(|(vid, c)| c.spot().map(|s| (s.pos.distance(p), vid)))
        .filter(|(d, _)| *d < 3.5)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, vid)) = parked {
        return Selection::Parked(vid);
    }
    // [road-inspect] The street piece under the pointer (not in a junction).
    if let Some(sid) = crate::road_select::pick(w, p, depth) {
        return Selection::Road(sid);
    }
    for (_, lot) in w.lots.iter() {
        if let Some(b) = lot.building {
            if point_in_polygon(p, &lot.polygon) {
                return Selection::Building(b);
            }
        }
    }
    Selection::None
}
