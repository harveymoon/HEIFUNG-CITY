//! What a click with the Inspect tool picks (`crate::selection::apply_pick`
//! makes it the selection).
//!
//! [feat/pt-ux] A click is a ray from the eye: an aircraft or a building
//! the ray meets before the ground wins (a tower is picked by its shape,
//! not by the ground behind it), and from far out people, robots and
//! parked cars are not picked at all ([`reach`]): the player means the
//! building or the street.

use bevy::math::Vec3;
use nd_sim::zoning::point_in_polygon;
use nd_sim::{Activity, BuildingId, LandUse, Vec2 as SVec2, World};

use super::Depth;
use crate::selection::Selection;
use crate::sim::Sim;

/// Farthest the eye may be from the ground under the pointer for people,
/// delivery robots and parked cars to be picked.
pub(crate) const PEOPLE_REACH: f32 = 320.0;
/// The same for cars on the road (buses, trains, ships and aircraft are
/// picked from any distance).
pub(crate) const CAR_REACH: f32 = 700.0;

/// What may be picked from this far out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Reach {
    /// People, firefighters, delivery robots and parked cars.
    pub small: bool,
    /// Cars and trucks on the road.
    pub cars: bool,
    /// How near a person must be to the click, metres: a person is a few
    /// pixels across, so close up a click must land on them (in a park the
    /// lot is meant, not the walker passing), farther out it may be wider.
    pub person_r: f32,
}

/// [feat/pt-ux] What a click from `view_dist` metres (the eye to the
/// ground under the pointer) may pick. 0 is a click with no eye (the remote
/// API's `select {at}`): everything, people within 6 m.
pub(crate) fn reach(view_dist: f32) -> Reach {
    let person_r = if view_dist <= 0.0 { 6.0 } else { (1.6 + view_dist * 0.012).min(6.0) };
    Reach { small: view_dist <= PEOPLE_REACH, cars: view_dist <= CAR_REACH, person_r }
}

/// A click: the ground under the pointer (None: the pointer is on the sky),
/// the view layer, the ray from the eye (Bevy coordinates: y up, z is the
/// sim's y) and how far the eye is from that ground.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Click {
    pub ground: Option<SVec2>,
    pub depth: Depth,
    pub ray: Option<(Vec3, Vec3)>,
    pub view_dist: f32,
}

impl Click {
    /// A click right at `p` from close by and straight above (the remote
    /// API's `select {at}`): everything there can be picked.
    pub fn at(p: SVec2, depth: Depth) -> Self {
        Click { ground: Some(p), depth, ray: None, view_dist: 0.0 }
    }
}

/// What a click at `p` picks: the vehicle drawn under it (tested where it
/// is drawn this frame, not where the sim will have it at the end of the
/// tick), else a person in the open, else a parked car, else a building.
/// The underground view draws only what is below ground, so there a click
/// picks only a vehicle in a tunnel: nothing it could not see.
pub(crate) fn pick(sim: &Sim, p: SVec2, depth: Depth) -> Selection {
    pick_click(sim, &Click::at(p, depth))
}

/// [feat/pt-ux] A click with its ray: aircraft and buildings in the way
/// first, then what is on the ground there as far as [`reach`] allows.
pub(crate) fn pick_click(sim: &Sim, click: &Click) -> Selection {
    if let Some(sel) = pick_on_ray(sim, click) {
        return sel;
    }
    // On the sky only an aircraft or a tower could be hit.
    let Some(ground) = click.ground else { return Selection::None };
    // [fire-scene] An emergency unit's car is the unit (its card follows it
    // from the road onto the scene and back).
    match pick_any(sim, ground, click.depth, reach(click.view_dist)) {
        Selection::Car(id) => sim.world.services.unit_of_car(id).map_or(Selection::Car(id), Selection::Unit),
        s => s,
    }
}

/// An aircraft or a building the ray meets before it reaches the ground.
fn pick_on_ray(sim: &Sim, click: &Click) -> Option<Selection> {
    let (o, d) = click.ray?;
    if click.depth == Depth::Underground {
        return None;
    }
    let w = &sim.world;
    let d = d.normalize_or_zero();
    let ground_t = click.ground.map_or(f32::INFINITY, |g| (Vec3::new(g.x, w.surface_height(g), g.y) - o).length());
    let air = crate::air_select::pick_ray(w, sim.frac(), o, d).filter(|(t, _)| *t < ground_t + 2.0);
    let bld = building_on_ray(w, &sim.building_tops, o, d).filter(|(t, _)| *t < ground_t - 0.5);
    // [K34] A shipping container in a depot's stacks or on a train, close up.
    if let Some((tb, id)) = crate::box_select::pick_ray(sim, o, d).filter(|(t, _)| *t < ground_t + 1.0 && *t < 160.0) {
        if air.is_none_or(|(ta, _)| tb < ta) {
            return Some(Selection::Box(id));
        }
    }
    match (air, bld) {
        (Some((ta, a)), Some((tb, _))) if ta <= tb => Some(Selection::Drone(a)),
        (Some((_, a)), None) => Some(Selection::Drone(a)),
        (_, Some((_, b))) => Some(Selection::Building(b)),
        _ => None,
    }
}

/// [feat/pt-ux] A building as a prism for picking and the pulse: its
/// footprint pulled in by the land use's setback, from the ground to its
/// drawn height. None for parks and plazas (flat: picked on the ground).
pub(crate) fn building_prism(w: &World, tops: &crate::fire_scene::BuildingTops, b: BuildingId) -> Option<(Vec<SVec2>, f32, f32)> {
    let bld = w.buildings.get(b)?;
    if bld.land_use == LandUse::Park {
        return None;
    }
    let lot = w.footprint(b)?;
    if lot.polygon.len() < 3 {
        return None;
    }
    let rules = crate::fire_scene::default_rules();
    let inset = rules.for_land_use(bld.land_use).inset;
    let c = lot.center;
    let poly: Vec<SVec2> = lot.polygon.iter().map(|&p| p + (c - p).normalize_or_zero() * inset.min(p.distance(c) * 0.5)).collect();
    let height = tops.height(w, b, rules);
    if height < 1.5 {
        return None;
    }
    // The lot's levelled pad, as the renderer stands the building on it.
    let base = w.lot_pad(bld.lot);
    Some((poly, base, base + height))
}

/// The nearest building whose prism the ray `o + d t` (`d` a unit vector)
/// enters, and `t`.
fn building_on_ray(w: &World, tops: &crate::fire_scene::BuildingTops, o: Vec3, d: Vec3) -> Option<(f32, BuildingId)> {
    let o2 = SVec2::new(o.x, o.z);
    let d2 = SVec2::new(d.x, d.z).normalize_or_zero();
    let mut best: Option<(f32, BuildingId)> = None;
    for (id, _) in w.buildings.iter() {
        let Some(lot) = w.footprint(id) else { continue };
        // Cheap reject: the lot's centre far off the ray's line on the map.
        if d2 != SVec2::ZERO {
            let rel = lot.center - o2;
            let along = rel.dot(d2).max(0.0);
            let off = (rel - d2 * along).length();
            let radius = lot.polygon.iter().map(|p| p.distance(lot.center)).fold(0.0, f32::max);
            if off > radius + 1.0 {
                continue;
            }
        }
        let Some((poly, base, top)) = building_prism(w, tops, id) else { continue };
        if let Some(t) = ray_prism(o, d, &poly, base, top) {
            if best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t, id));
            }
        }
    }
    best
}

/// [feat/pt-ux] Where the ray `o + d t` (Bevy coordinates) first enters
/// the upright prism over `poly` (sim x, y) from `base` to `top`: its roof,
/// floor or a wall. None when it misses or starts inside.
pub(crate) fn ray_prism(o: Vec3, d: Vec3, poly: &[SVec2], base: f32, top: f32) -> Option<f32> {
    let o2 = SVec2::new(o.x, o.z);
    let d2 = SVec2::new(d.x, d.z);
    if o.y >= base && o.y <= top && point_in_polygon(o2, poly) {
        return None;
    }
    let mut best = f32::MAX;
    // The roof (from above) and the floor (from below).
    for (y, from_above) in [(top, true), (base, false)] {
        if (from_above && d.y < -1e-6 && o.y > y) || (!from_above && d.y > 1e-6 && o.y < y) {
            let t = (y - o.y) / d.y;
            if t > 0.0 && point_in_polygon(o2 + d2 * t, poly) {
                best = best.min(t);
            }
        }
    }
    // The walls.
    for i in 0..poly.len() {
        let (a, c) = (poly[i], poly[(i + 1) % poly.len()]);
        let e = c - a;
        let den = d2.x * e.y - d2.y * e.x;
        if den.abs() < 1e-9 {
            continue;
        }
        let r = a - o2;
        let t = (r.x * e.y - r.y * e.x) / den;
        let s = (r.x * d2.y - r.y * d2.x) / den;
        if t > 0.0 && (0.0..=1.0).contains(&s) {
            let y = o.y + d.y * t;
            if y >= base && y <= top {
                best = best.min(t);
            }
        }
    }
    (best < f32::MAX).then_some(best)
}

fn pick_any(sim: &Sim, p: SVec2, depth: Depth, reach: Reach) -> Selection {
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
        // [feat/pt-ux] From far out only buses (a car is a speck there).
        .filter(|(_, _, car)| reach.cars || car.bus.is_some())
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
    if let Some((_, c)) = crate::fire_scene::pick_crew(w, &sim.building_tops, p, f).filter(|(d, _)| reach.small && *d < 1.0) {
        return Selection::Citizen(c);
    }
    if let Some((du, uid)) = crate::fire_scene::pick_standing(w, p).filter(|_| reach.cars) {
        if car.is_none_or(|c| c.0 > du) {
            return Selection::Unit(uid);
        }
    }
    // People in the open (drivers are picked through their car).
    let person = w
        .citizens
        .iter()
        .filter(|(_, c)| reach.small && c.is_visible() && !matches!(c.activity, Activity::Driving { .. }))
        .map(|(id, c)| (c.prev_pos.lerp(c.pos, f).distance(p), id))
        .filter(|(d, _)| *d < reach.person_r)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    // [camera] Pavement delivery robots: small, so a click must be close,
    // and nearer than any car or person it might be standing by.
    let bot = w
        .bots
        .bots
        .iter()
        .map(|(id, b)| ((if b.prev_pos.distance(b.pos) < 30.0 { b.prev_pos.lerp(b.pos, f) } else { b.pos }).distance(p), id))
        .filter(|(d, _)| reach.small && *d < 2.0)
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
    // [feat/pt-ux] A household living in its camper or van at the kerb.
    if let Some(seed) = crate::rv_select::pick(w, p).filter(|_| reach.cars) {
        return Selection::Rv(seed);
    }
    // Parked cars at the kerb. [feat/pt-ux] Only those drawn there (F42: a
    // car in a lot or a garage is not drawn on its own, and picking it gave
    // an empty box over nothing), and only from close by.
    let parked = w
        .vehicles
        .cars
        .iter()
        .filter(|_| reach.small)
        .filter_map(|(vid, c)| c.spot().filter(|s| s.kind.on_street() || w.in_park_car_park(s)).map(|s| (s.pos.distance(p), vid)))
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
    // [feat/pt-ux] F28: a park plot of its own has no building (a park of
    // several plots is one building, on the plot by the street): the park
    // it is part of.
    if let Some((id, _)) = w.lots.iter().find(|(_, l)| l.building.is_none() && l.land_use == LandUse::Park && point_in_polygon(p, &l.polygon)) {
        if let Some(b) = park_of_plot(w, id) {
            return Selection::Building(b);
        }
    }
    // [feat/pt-ux] From far out a click on the pavement between plots
    // means the plot beside it (a few pixels off is metres there).
    if !reach.small {
        let near = w
            .lots
            .values()
            .filter_map(|lot| lot.building.map(|b| (edge_distance(p, &lot.polygon), b)))
            .filter(|(d, _)| *d < FAR_SLACK)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, b)) = near {
            return Selection::Building(b);
        }
    }
    Selection::None
}

/// [feat/pt-ux] The park building of the park the plot `start` belongs to:
/// over touching park plots to the first with a building.
pub(crate) fn park_of_plot(w: &World, start: nd_sim::LotId) -> Option<BuildingId> {
    let parks: Vec<(nd_sim::LotId, &nd_sim::Lot)> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Park && l.polygon.len() >= 3).collect();
    let touch = |a: &nd_sim::Lot, b: &nd_sim::Lot| a.polygon.iter().any(|&p| edge_distance(p, &b.polygon) < 1.0) || b.polygon.iter().any(|&p| edge_distance(p, &a.polygon) < 1.0);
    let mut seen = vec![start];
    let mut queue = std::collections::VecDeque::from([start]);
    while let Some(id) = queue.pop_front() {
        let lot = w.lots.get(id)?;
        if let Some(b) = lot.building.filter(|b| w.buildings.get(*b).is_some_and(|x| x.land_use == LandUse::Park)) {
            return Some(b);
        }
        for &(other, l) in &parks {
            if !seen.contains(&other) && touch(lot, l) {
                seen.push(other);
                queue.push_back(other);
            }
        }
    }
    None
}

/// How far off a plot's edge a click from far out still picks it.
const FAR_SLACK: f32 = 8.0;

/// The distance from `p` to the nearest edge of `poly`.
fn edge_distance(p: SVec2, poly: &[SVec2]) -> f32 {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
            (a + ab * t).distance(p)
        })
        .fold(f32::MAX, f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(c: SVec2, h: f32) -> Vec<SVec2> {
        vec![c + SVec2::new(-h, -h), c + SVec2::new(h, -h), c + SVec2::new(h, h), c + SVec2::new(-h, h)]
    }

    #[test]
    fn reach_drops_small_things_then_cars_as_the_eye_pulls_back() {
        let r = |d: f32| (reach(d).small, reach(d).cars);
        assert_eq!(r(50.0), (true, true));
        assert_eq!(r(PEOPLE_REACH), (true, true));
        assert_eq!(r(PEOPLE_REACH + 1.0), (false, true));
        assert_eq!(r(CAR_REACH + 1.0), (false, false));
        // Close up a click must land on the person; no eye (the API), 6 m.
        assert!(reach(40.0).person_r < 2.5);
        assert!(reach(200.0).person_r > reach(40.0).person_r);
        assert_eq!(reach(0.0).person_r, 6.0);
        // Monotone: once out of reach, farther stays out.
        let mut last = reach(0.0);
        for d in (0..2000).step_by(10) {
            let r = reach(d as f32);
            assert!(!(r.small && !last.small) && !(r.cars && !last.cars));
            last = r;
        }
    }

    #[test]
    fn a_ray_at_a_tower_wall_hits_it_before_the_ground_behind() {
        // A 10 m square tower 100 m tall at (0, 0); the eye 200 m south, 50 m up,
        // looking north and a little down.
        let poly = square(SVec2::ZERO, 5.0);
        let o = Vec3::new(0.0, 50.0, -200.0);
        let d = Vec3::new(0.0, -0.1, 1.0).normalize();
        let t = ray_prism(o, d, &poly, 0.0, 100.0).expect("hits the south wall");
        let hit = o + d * t;
        assert!((hit.z + 5.0).abs() < 1e-3, "on the wall at z = -5, got {hit:?}");
        // The same ray meets the ground (y = 0) far behind the tower.
        let ground_t = -o.y / d.y;
        assert!(t < ground_t);
    }

    #[test]
    fn a_ray_over_a_low_roof_misses_and_one_onto_it_hits_the_roof() {
        let poly = square(SVec2::new(0.0, 0.0), 5.0);
        let o = Vec3::new(0.0, 100.0, -100.0);
        // Aimed at the ground well past it: passes over the 6 m roof.
        let over = (Vec3::new(0.0, 0.0, 60.0) - o).normalize();
        assert!(ray_prism(o, over, &poly, 0.0, 6.0).is_none());
        // Straight down onto the roof from above it.
        let down = ray_prism(Vec3::new(1.0, 50.0, 1.0), Vec3::NEG_Y, &poly, 0.0, 6.0).unwrap();
        assert!((down - 44.0).abs() < 1e-3);
        // Aimed away: nothing.
        assert!(ray_prism(o, -over, &poly, 0.0, 6.0).is_none());
    }

    #[test]
    fn the_nearer_of_two_towers_on_the_ray_is_hit_first() {
        let near = square(SVec2::new(0.0, -50.0), 5.0);
        let far = square(SVec2::new(0.0, 50.0), 5.0);
        let o = Vec3::new(0.0, 30.0, -300.0);
        let d = Vec3::Z;
        let tn = ray_prism(o, d, &near, 0.0, 80.0).unwrap();
        let tf = ray_prism(o, d, &far, 0.0, 80.0).unwrap();
        assert!(tn < tf);
        // Above the near tower's roof, only the far (taller) one.
        let o2 = Vec3::new(0.0, 90.0, -300.0);
        assert!(ray_prism(o2, d, &near, 0.0, 80.0).is_none());
        assert!(ray_prism(o2, d, &far, 0.0, 120.0).is_some());
    }
}

#[cfg(test)]
mod edge_tests {
    use super::*;

    #[test]
    fn edge_distance_is_to_the_nearest_side() {
        let sq = [SVec2::new(0.0, 0.0), SVec2::new(10.0, 0.0), SVec2::new(10.0, 10.0), SVec2::new(0.0, 10.0)];
        assert!((edge_distance(SVec2::new(5.0, -3.0), &sq) - 3.0).abs() < 1e-4);
        assert!((edge_distance(SVec2::new(5.0, 2.0), &sq) - 2.0).abs() < 1e-4);
        assert!((edge_distance(SVec2::new(13.0, 14.0), &sq) - 5.0).abs() < 1e-4);
    }
}
