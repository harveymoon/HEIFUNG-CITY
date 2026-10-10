//! [overlay] The selected thing, drawn: one exhaustive match over
//! [`Selection`], so a new kind of selection has to say how it shows. A
//! building's lot raised off the ground, a box round a person or vehicle,
//! with what goes with each: a person's route, destination and places, a
//! car's route and the jam it waits in, a train's run and stops (from
//! `train_select`), a ship (from `ferry_select`), a toll booth, a street
//! piece, a delivery robot's route and shop.

use bevy::prelude::*;
use nd_sim::world::ferry_routes::WaterNet;
use nd_sim::{Activity, BuildingId, CarId, CitizenId, Vec2 as SVec2, World};

use super::shapes::{flat_at, lot_outline, person_box, route_ends, vehicle_box, GOLD};
use crate::selection::Selection;
use crate::sim::Sim;

/// What drawing the selection needs to know besides the sim.
pub struct SelectionCx<'a> {
    pub sim: &'a Sim,
    pub sel: Selection,
    /// Seconds since start, for pulses.
    pub t: f32,
    /// The renderer's car colours (a train's glow).
    pub palette: &'a [[f32; 3]],
    /// The Underground view.
    pub under: bool,
    /// The camera rides inside the followed vehicle (or with a driving or
    /// riding person): a box drawn round it would be drawn round the eye.
    pub pov_vehicle: bool,
    /// The walk camera follows anything at all.
    pub pov_walk: bool,
    /// The water network this frame, if worked out already.
    pub water: Option<&'a WaterNet>,
}

/// Draw the selection: every variant, no `_` arm.
pub fn draw_selection(gizmos: &mut Gizmos, cx: &SelectionCx) {
    let sim = cx.sim;
    let w = &sim.world;
    let f = sim.frac();
    // [camera] Not from inside the followed vehicle: its outline box would
    // be drawn round the eye.
    let outside = !cx.pov_vehicle;
    match cx.sel {
        Selection::None => {}
        Selection::Citizen(id) => {
            if outside {
                citizen(gizmos, sim, id);
            }
            // A selected person's places, from any camera.
            places(gizmos, w, id);
        }
        Selection::Building(b) if outside => building(gizmos, sim, b, cx.t),
        Selection::Building(_) => {}
        Selection::Car(id) if outside => car(gizmos, w, f, id),
        Selection::Car(_) => {}
        Selection::Parked(vid) if outside => {
            let Some(spot) = w.vehicles.cars.get(vid).and_then(|c| c.spot()) else { return };
            vehicle_box(gizmos, spot.pos, spot.heading, 5.2, 2.6, w.surface_height(spot.pos) + 0.35, GOLD);
        }
        Selection::Parked(_) => {}
        Selection::Toll(id) if outside => crate::toll_select::draw(gizmos, w, id),
        Selection::Toll(_) => {}
        // [camera] Hidden whenever the walk camera follows anything.
        Selection::Bot(id) if !cx.pov_walk => bot(gizmos, w, f, id),
        Selection::Bot(_) => {}
        // [train-inspect] A train: its cars, the run ahead in its colour, its stops.
        Selection::Train(_) | Selection::Metro(_) if outside => crate::train_select::draw(gizmos, sim, cx.sel, cx.palette, cx.under, cx.t),
        Selection::Train(_) | Selection::Metro(_) => {}
        Selection::Road(sid) if outside => crate::road_select::draw(gizmos, w, sid),
        Selection::Road(_) => {}
        Selection::Unit(uid) if outside => unit(gizmos, w, f, uid),
        Selection::Unit(_) => {}
        // [ferry-routes] Drawn from any camera (its route is `ferry_select`'s).
        Selection::Ship(id) => match cx.water {
            Some(net) => crate::ferry_select::draw_ship(gizmos, w, net, id, f),
            None => crate::ferry_select::draw_ship(gizmos, w, &crate::ferry_select::net(w).1, id, f),
        },
        // [feat/pt-ux] Not from inside the cabin, nor underground.
        Selection::Drone(id) if outside && !cx.under => crate::air_select::draw(gizmos, w, id, f, cx.t),
        Selection::Drone(_) => {}
        Selection::Rv(seed) if outside && !cx.under => crate::rv_select::draw(gizmos, w, seed),
        Selection::Rv(_) => {}
    }
}

const ROUTE: Color = Color::linear_rgb(0.5, 2.6, 3.2);
const TRAIL: Color = Color::linear_rgb(0.18, 0.55, 0.75);
const DEST: Color = Color::linear_rgb(2.8, 1.0, 3.0);

/// Heights are above the ground there, so lines never sink into hills.
fn at(w: &World, p: SVec2, y: f32) -> Vec3 {
    Vec3::new(p.x, y + w.surface_height(p), p.y)
}

/// The same blend as the renderer, so the ring sits under the figure.
fn blend(f: f32, a: SVec2, b: SVec2) -> SVec2 {
    if a.distance(b) < 30.0 { a.lerp(b, f) } else { b }
}

/// [fire-scene] A unit: its car's outline and route while it drives;
/// standing, its body, and the building it is working at.
fn unit(gizmos: &mut Gizmos, w: &World, f: f32, uid: nd_sim::UnitId) {
    let Some(u) = w.services.units.get(uid) else { return };
    if let Some(c) = u.car.filter(|c| w.traffic.cars.contains_key(*c)) {
        car(gizmos, w, f, c);
    } else if let Some((p, h)) = crate::fire_scene::unit_pose(w, uid, f) {
        let (len, wid, tall) = match u.kind {
            nd_sim::UnitKind::FireEngine => (8.4, 2.9, 3.2),
            nd_sim::UnitKind::Ambulance => (6.3, 2.6, 2.7),
            nd_sim::UnitKind::PoliceCar => (5.0, 2.3, 1.9),
        };
        vehicle_box(gizmos, p, h, len, wid, tall + w.surface_height(p), GOLD);
    }
    if let Some(i) = crate::fire_scene::unit_incident(w, uid).filter(|i| w.buildings.contains_key(i.building)) {
        lot_outline(gizmos, w, i.building, 1.3, Color::linear_rgb(3.0, 0.9, 0.2));
    }
}

fn building(gizmos: &mut Gizmos, sim: &Sim, b: BuildingId, t: f32) {
    let w = &sim.world;
    lot_outline(gizmos, w, b, 1.6, GOLD);
    // [feat/pt-ux] The whole building, not only its plot: its shape in
    // gold, a band of light running up it and the edges pulsing, so a
    // tower picked by its walls reads as picked from anywhere.
    building_pulse(gizmos, sim, b, t);
    // [plant-groups] A Sunfield or Wind Park is selected whole.
    if let Some(g) = w.plant_group_of(b) {
        for m in g.members.into_iter().filter(|&m| m != b) {
            lot_outline(gizmos, w, m, 1.6, GOLD);
        }
    }
    // [land] Everything its owner owns (a rich person's other homes
    // and businesses), outlined and tied to it by a line.
    let owned = w.owned_with(b);
    if owned.len() > 1 {
        let teal = Color::linear_rgb(0.4, 2.6, 2.2);
        let from = w.footprint(b).map(|l| l.center);
        for p in owned.into_iter().skip(1) {
            lot_outline(gizmos, w, p, 1.3, teal);
            if let (Some(a), Some(c)) = (from, w.footprint(p).map(|l| l.center)) {
                gizmos.line(at(w, a, 6.0), at(w, c, 6.0), teal);
            }
        }
    }
}

/// [feat/pt-ux] The selected building's prism (`tools::building_prism`):
/// footprint at the foot and the roof, the corners, and a ring of light
/// sweeping up it, all breathing with `t`.
fn building_pulse(gizmos: &mut Gizmos, sim: &Sim, b: BuildingId, t: f32) {
    let Some((poly, base, top)) = crate::tools::building_prism(&sim.world, &sim.building_tops, b) else { return };
    let breathe = 0.55 + 0.45 * (t * 3.2).sin();
    let edge = Color::linear_rgb(3.2 * breathe + 0.6, 2.4 * breathe + 0.4, 0.7 * breathe + 0.1);
    let at = |p: SVec2, y: f32| Vec3::new(p.x, y, p.y);
    // Pushed out a hair so the lines are not lost in the walls.
    let c = poly.iter().copied().fold(SVec2::ZERO, |a, p| a + p) / poly.len() as f32;
    let poly: Vec<SVec2> = poly.iter().map(|&p| p + (p - c).normalize_or_zero() * 0.35).collect();
    let (base, top) = (base + 0.1, top + 0.3);
    for y in [base, top] {
        super::shapes::outline(gizmos, &poly, |p| at(p, y), edge);
    }
    for &p in &poly {
        gizmos.line(at(p, base), at(p, top), edge);
    }
    // Floors' worth of faint rings, and one bright one climbing.
    let h = top - base;
    let rings = ((h / 12.0) as usize).clamp(1, 14);
    for i in 1..rings {
        let y = base + h * i as f32 / rings as f32;
        super::shapes::outline(gizmos, &poly, |p| at(p, y), GOLD.with_alpha(0.25 * breathe));
    }
    let sweep = base + h * (t * 0.45).fract();
    super::shapes::outline(gizmos, &poly, |p| at(p, sweep), Color::linear_rgb(4.0, 3.2, 1.2));
}

fn car(gizmos: &mut Gizmos, w: &World, f: f32, id: CarId) {
    let Some(car) = w.traffic.cars.get(id) else { return };
    let (p, heading) = car.pose_at(f);
    let width = if car.bus.is_some() || car.cargo.is_some() { 2.7 } else { 2.1 };
    vehicle_box(gizmos, p, heading, car.length() + 0.6, width + 0.5, w.car_height(car) + 0.35, GOLD);
    car_route(gizmos, w, car, ROUTE);
    // [clarity] An "o" where it set off, an "x" where it is going.
    let origin = w.car_journey(id).and_then(|j| j.from.pos);
    route_ends(gizmos, w, origin, car_route_end(w, car));
    // [services] A wreck with its recovery truck on the way: the
    // truck's road to it, in amber, and a ring round the truck.
    if let Some(t) = w.driving.wrecks.iter().find(|x| x.car == id).and_then(|x| x.truck).and_then(|t| w.traffic.cars.get(t)) {
        let amber = Color::linear_rgb(3.0, 1.4, 0.1);
        car_route(gizmos, w, t, amber);
        let (tp, th) = t.pose_at(f);
        vehicle_box(gizmos, tp, th, t.length() + 0.6, 2.6, w.car_height(t) + 0.35, amber);
    }
    // Where it is going: the cargo's buyer, or the driver's errand.
    let to = car.cargo.as_ref().map(|c| c.to).or_else(|| match w.citizens.get(car.citizen).map(|c| &c.activity) {
        Some(Activity::Driving { to, .. }) => *to,
        _ => None,
    });
    if let Some(b) = to {
        lot_outline(gizmos, w, b, 1.2, DEST);
    }
    // Who it waits for, back to the head of the jam: a red line from
    // car to car and a box round the head (red for a car standing
    // for no reason the sim knows, orange otherwise).
    let chain = w.traffic.jam_chain(id);
    if chain.cars.len() >= 2 {
        let jam = Color::linear_rgb(3.0, 0.35, 0.25);
        let pts: Vec<Vec3> = chain.cars.iter().filter_map(|&c| w.traffic.cars.get(c)).map(|c| {
            let p = c.pose_at(f).0;
            Vec3::new(p.x, w.car_height(c) + 1.6, p.y)
        }).collect();
        gizmos.linestrip(pts, jam);
        for &c in &chain.cars[1..chain.cars.len() - 1] {
            if let Some(x) = w.traffic.cars.get(c) {
                let (p, h) = x.pose_at(f);
                vehicle_box(gizmos, p, h, x.length() + 0.3, 2.3, w.car_height(x) + 0.3, Color::linear_rgb(1.4, 0.25, 0.2));
            }
        }
        if let Some(head) = chain.head().and_then(|h| w.traffic.cars.get(h).map(|x| (h, x))) {
            let (p, h) = head.1.pose_at(f);
            let bad = !w.traffic.stop_reason(head.0).is_good();
            let col = if bad { Color::linear_rgb(4.0, 0.2, 0.2) } else { Color::linear_rgb(3.4, 1.4, 0.2) };
            vehicle_box(gizmos, p, h, head.1.length() + 1.0, 3.0, w.car_height(head.1) + 0.6, col);
        }
    }
}

/// A person: where they are (or the vehicle they are in), the road ahead
/// (the rest of their route, the lane path of their car or bus, the line
/// to their station), the destination's lot, and the route so far, dimmer.
fn citizen(gizmos: &mut Gizmos, sim: &Sim, id: CitizenId) {
    let w = &sim.world;
    let f = sim.frac();
    let at = |p: SVec2, y: f32| at(w, p, y);
    let Some(c) = w.citizens.get(id) else { return };
    let here = blend(f, c.prev_pos, c.pos);
    if !c.trail.is_empty() {
        let mut pts: Vec<Vec3> = c.trail.iter().map(|&q| at(q, 0.7)).collect();
        pts.push(at(here, 0.7));
        if pts.len() >= 2 {
            gizmos.linestrip(pts, TRAIL);
        }
    }
    let mut destination = c.transit.map(|t| t.target);
    match &c.activity {
        Activity::Traveling { to, path, idx, .. } => {
            destination = to.or(destination);
            let mut pts = vec![at(here, 1.0)];
            pts.extend(path.iter().skip(*idx).map(|&q| at(q, 1.0)));
            if pts.len() >= 2 {
                gizmos.linestrip(pts.iter().map(|p| *p + Vec3::Y * 0.4), ROUTE);
                gizmos.linestrip(pts, ROUTE);
            }
            person_box(gizmos, at(here, 0.0), GOLD);
        }
        Activity::Driving { car, to, .. } => {
            destination = to.or(destination);
            if let Some(car) = w.traffic.cars.get(*car) {
                // At the wheel: the car's outline and its route, as a
                // selected car shows.
                let (p, heading) = car.pose_at(f);
                let width = if car.bus.is_some() || car.cargo.is_some() { 2.7 } else { 2.1 };
                vehicle_box(gizmos, p, heading, car.length() + 0.6, width + 0.5, w.car_height(car) + 0.35, GOLD);
                car_route(gizmos, w, car, ROUTE);
            }
        }
        Activity::Flying { drone } => {
            if let Some(d) = w.air.drones.get(*drone) {
                let p = blend(f, d.prev_pos, d.pos);
                let alt = d.prev_alt + (d.alt - d.prev_alt) * f;
                gizmos.circle(flat_at(at(p, alt)), 6.0, GOLD);
                gizmos.line(at(p, alt), at(d.goal, d.goal_alt + 1.0), ROUTE);
            }
        }
        // [passengers] In someone's car: the car and its route.
        Activity::Passenger { driver, to, seated, .. } => {
            destination = to.or(destination);
            let car = w.citizens.get(*driver).and_then(|d| match d.activity {
                Activity::Driving { car, .. } => w.traffic.cars.get(car),
                _ => None,
            });
            match car.filter(|_| *seated) {
                Some(car) => {
                    let (p, heading) = car.pose_at(f);
                    vehicle_box(gizmos, p, heading, car.length() + 0.6, 2.6, w.car_height(car) + 0.35, GOLD);
                    car_route(gizmos, w, car, ROUTE);
                }
                None => person_box(gizmos, at(here, 0.0), GOLD),
            }
        }
        // [ferries] Away off the map: nothing to draw.
        Activity::Abroad { .. } => {}
        Activity::Riding { bus } => {
            if let Some(bus) = w.traffic.cars.get(*bus) {
                let p = bus.pose_at(f).0;
                gizmos.circle(flat_at(at(p, 0.5)), 7.5, GOLD);
                car_route(gizmos, w, bus, ROUTE);
            }
        }
        // [subway] Underground: a ring where the train is, its line ahead.
        Activity::OnMetro { train } => {
            if let Some(t) = w.subway.trains.get(*train) {
                let s = t.run.prev_s + (t.run.s - t.run.prev_s) * f;
                let head = nd_sim::rail::point_along(&t.run.poly, s).0;
                gizmos.circle(flat_at(at(head, 0.5)), 8.0, GOLD);
                let alight = match c.transit.map(|t| t.leg) {
                    Some(nd_sim::transit::TransitLeg::Subway { alight, .. }) => Some(alight),
                    _ => None,
                };
                let s_end = alight.and_then(|b| t.run.stops.iter().find(|(stop, _)| *stop == b)).map_or_else(|| nd_sim::rail::path_length(&t.run.poly), |&(_, s1)| s1);
                let pts: Vec<Vec3> = sub_polyline(&t.run.poly, s, s_end).iter().map(|&q| at(q, 1.0)).collect();
                if pts.len() >= 2 {
                    gizmos.linestrip(pts, ROUTE);
                }
            }
        }
        Activity::OnTrain { train } => {
            if let Some(t) = w.rail.trains.get(*train) {
                let s = t.prev_s + (t.s - t.prev_s) * f;
                let head = nd_sim::rail::point_along(&t.poly, s).0;
                gizmos.circle(flat_at(at(head, 0.5)), 8.0, GOLD);
                // The line ahead as far as the rider's station.
                let alight = match c.transit.map(|t| t.leg) {
                    Some(nd_sim::transit::TransitLeg::Rail { alight, .. }) => Some(alight),
                    _ => None,
                };
                let s_end = alight.and_then(|b| t.stops.iter().find(|(stop, _)| *stop == b)).map_or_else(|| nd_sim::rail::path_length(&t.poly), |&(_, s1)| s1);
                let pts: Vec<Vec3> = sub_polyline(&t.poly, s, s_end).iter().map(|&q| at(q, 1.0)).collect();
                if pts.len() >= 2 {
                    gizmos.linestrip(pts, ROUTE);
                }
            }
        }
        Activity::Waiting { at: q } => {
            person_box(gizmos, at(*q, 0.0), GOLD);
        }
        Activity::Idle | Activity::Sitting { .. } => {
            person_box(gizmos, at(here, 0.0), GOLD);
        }
        Activity::AtHome | Activity::Working | Activity::Visiting(_) | Activity::Arriving => {
            // Indoors: show the building they are in.
            if let Some(b) = c.inside {
                lot_outline(gizmos, w, b, 1.6, GOLD);
            }
        }
    }
    // [clarity] An "o" where the trip began, an "x" at its end.
    let origin = c.trail.first().copied().or_else(|| w.citizen_origin(id).and_then(|p| p.pos));
    let end = match &c.activity {
        Activity::Traveling { path, .. } => path.last().copied(),
        Activity::Driving { car, .. } => w.traffic.cars.get(*car).and_then(|car| car_route_end(w, car)),
        _ => None,
    }
    .or_else(|| destination.and_then(|b| w.building_entrance(b)));
    if c.activity.is_traveling() || matches!(c.activity, Activity::Driving { .. } | Activity::Passenger { .. } | Activity::Riding { .. } | Activity::Waiting { .. }) {
        route_ends(gizmos, w, origin, end);
    }
    if let Some(b) = destination {
        lot_outline(gizmos, w, b, 1.2, DEST);
        // A thin line from where they are to where they are going,
        // so the goal reads even when the route turns away from it.
        if let Some(goal) = w.buildings.get(b).and_then(|b| w.lots.get(b.lot)).map(|l| l.center) {
            let faint = Color::linear_rgb(1.2, 0.4, 1.3);
            gizmos.line(at(here, 3.0), at(goal, 3.0), faint);
        }
    }
}

/// [camera] A selected person's places: a post from each lot (home, work,
/// a business they own) up to its pin, in the holding's colour (the pins'
/// heads are drawn by `ui::pins`).
fn places(gizmos: &mut Gizmos, w: &World, id: CitizenId) {
    for (b, h) in w.holdings_of(id) {
        let Some(top) = crate::marks::pin_top(w, b) else { continue };
        let [r, g, bl] = crate::marks::holding_rgb(h);
        let col = Color::srgb_u8(r, g, bl).to_linear() * 3.0;
        let col = Color::LinearRgba(col);
        let foot = Vec3::new(top.x, w.surface_height(SVec2::new(top.x, top.z)), top.z);
        gizmos.line(foot, top - Vec3::Y * 4.0, col);
        gizmos.circle(flat_at(foot + Vec3::Y * 0.5), 4.0, col);
        lot_outline(gizmos, w, b, 1.0, col);
    }
}

/// [camera] A delivery robot: its outline, the pavement ahead in its
/// fleet's colour, the door it is heading for and the shop it belongs to.
fn bot(gizmos: &mut Gizmos, w: &World, f: f32, id: nd_sim::BotId) {
    // On the level walkers and robots are drawn on (under a viaduct, not on it).
    let at = |p: SVec2, y: f32| Vec3::new(p.x, y + w.surface_height_near(p, w.ground_height(p)), p.y);
    let Some(b) = w.bots.bots.get(id) else { return };
    let [r, g, bl] = nd_sim::bots::fleet_colour(b.shop);
    let fleet = Color::linear_rgb(r * 3.0, g * 3.0, bl * 3.0);
    let p = blend(f, b.prev_pos, b.pos);
    let heading = b.prev_heading + (b.heading - b.prev_heading) * f;
    vehicle_box(gizmos, p, heading, 1.4, 1.0, at(p, 0.1).y, GOLD);
    let mut pts: Vec<Vec3> = vec![at(p, 0.6)];
    pts.extend(b.path.iter().skip(b.idx).map(|&q| at(q, 0.6)));
    if pts.len() >= 2 {
        gizmos.linestrip(pts.iter().map(|v| *v + Vec3::Y * 0.3), fleet);
        gizmos.linestrip(pts, fleet);
    }
    lot_outline(gizmos, w, b.destination(), 1.2, DEST);
    if b.destination() != b.shop {
        lot_outline(gizmos, w, b.shop, 1.0, fleet);
    }
}

/// Where a car's lane route ends.
fn car_route_end(w: &World, car: &nd_sim::Car) -> Option<SVec2> {
    let net = &w.traffic.net;
    if car.idx >= car.route.len() || car.route.iter().any(|&l| l as usize >= net.lanes.len()) {
        return None;
    }
    net.polyline(&car.route[car.idx..], car.s, car.s_end).last().copied()
}

/// The rest of a car's lane route, from where it is to where it stops.
fn car_route(gizmos: &mut Gizmos, w: &World, car: &nd_sim::Car, col: Color) {
    let net = &w.traffic.net;
    // Routes outlive lane rebuilds for a tick; never index past the lanes.
    if car.idx >= car.route.len() || car.route.iter().any(|&l| l as usize >= net.lanes.len()) {
        return;
    }
    let pts: Vec<Vec3> = net.polyline(&car.route[car.idx..], car.s, car.s_end).iter().map(|&q| Vec3::new(q.x, 1.0 + w.surface_height(q), q.y)).collect();
    if pts.len() >= 2 {
        gizmos.linestrip(pts, col);
    }
}

/// The stretch of a polyline between two arc lengths.
fn sub_polyline(pts: &[SVec2], s0: f32, s1: f32) -> Vec<SVec2> {
    let (s0, s1) = if s0 <= s1 { (s0, s1) } else { (s1, s0) };
    let mut out = vec![nd_sim::rail::point_along(pts, s0).0];
    let mut acc = 0.0;
    for w in pts.windows(2) {
        acc += w[0].distance(w[1]);
        if acc > s0 && acc < s1 {
            out.push(w[1]);
        }
    }
    out.push(nd_sim::rail::point_along(pts, s1).0);
    out
}
