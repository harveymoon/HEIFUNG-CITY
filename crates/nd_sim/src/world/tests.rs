//! World-level tests: cities built by hand, run for a while and checked.

use super::*;
use super::trips::VENUE_COVER;
use crate::market::Resource;
use crate::mind::IdleMind;

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

/// Open every building site at once, for tests about what happens after.
fn finish_all(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

/// The old flat map with no river or highway, for tests that count
/// gestures or depend on the exact random stream of a dry town.
fn flat_world() -> World {
    World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

#[test]
fn city_grows_and_round_trips_through_ron() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -400.0), Vec2::new(0.0, 400.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    assert!(!w.lots.is_empty());
    let mut minds = IdleMind;
    for _ in 0..(24 * 60) {
        w.tick(&mut minds);
    }
    assert!(!w.buildings.is_empty());
    finish_all(&mut w);
    for _ in 0..120 {
        w.tick(&mut minds);
    }
    assert!(!w.citizens.is_empty());
    let text = w.to_ron().unwrap();
    let back = World::from_ron(&text).unwrap();
    assert_eq!(back.citizens.len(), w.citizens.len());
    assert_eq!(back.traffic.net.lanes.len(), w.traffic.net.lanes.len());
}

#[test]
fn projects_undo_redo_and_keep_buildings_on_implement() {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    let lot = w.lots.iter().find(|(_, l)| l.usable()).map(|(id, _)| id).unwrap();
    w.build_on(lot);
    let building = w.lots[lot].building.unwrap();

    let id = w.add_gesture(Gesture::Road { points: vec![Vec2::new(300.0, -300.0), Vec2::new(300.0, 300.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
    assert_eq!(w.preview().planned_roads.len(), 1);
    assert!(w.undo());
    assert_eq!(w.preview().planned_roads.len(), 0);
    assert!(w.redo());
    w.set_gesture(id, Gesture::Road { points: vec![Vec2::new(320.0, -300.0), Vec2::new(320.0, 300.0)], lanes_fwd: 2, lanes_bwd: 2, curved: false, kind: RoadKind::Street, elevation: Vec::new() }, true);
    w.implement();
    assert!(!w.has_project());
    assert_eq!(w.master.gestures.len(), 4, "two roads, a zone and the trunk line");
    assert!(w.buildings.contains_key(building), "building on an untouched lot survives a rebuild");
    assert!(w.roads.alive_segments().count() >= 3);
    assert!(!w.traffic.net.intersections.is_empty());
}

#[test]
fn newest_zone_wins_and_repaint_changes_lots() {
    use crate::zoning::point_in_polygon;
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    let tri = vec![Vec2::new(-180.0, 10.0), Vec2::new(250.0, 10.0), Vec2::new(-180.0, 250.0)];
    let first = w.add_zone(tri.clone(), LandUse::Entertainment).unwrap();
    assert!(w.lots.values().all(|l| l.land_use == LandUse::Entertainment));
    // Drawing a commercial zone over it repaints the ground.
    w.add_zone(tri.clone(), LandUse::Commercial);
    assert!(w.lots.values().filter(|l| point_in_polygon(l.center, &tri)).all(|l| l.land_use == LandUse::Commercial));
    // Repainting the original gesture and implementing also works.
    assert!(w.repaint_zone(first, LandUse::Park));
    w.implement();
    // The newer commercial zone still sits on top of the repainted one
    // wherever shops may go; the backland a shop cannot use (no street)
    // refuses the commercial zone and stays park, which goes anywhere.
    assert!(w.lots.values().filter(|l| point_in_polygon(l.center, &tri) && l.usable()).all(|l| l.land_use == LandUse::Commercial));
    assert!(w.lots.values().filter(|l| point_in_polygon(l.center, &tri) && !l.usable()).all(|l| l.land_use == LandUse::Park));
}

#[test]
fn supply_chain_feeds_the_city() {
    let mut w = World::new(WorldConfig::test_town());
    for i in -1..=1 {
        let c = i as f32 * 200.0;
        w.add_road(&[Vec2::new(-500.0, c), Vec2::new(500.0, c)], 1, 1);
        w.add_road(&[Vec2::new(c, -500.0), Vec2::new(c, 500.0)], 1, 1);
    }
    w.add_zone(square(-190.0, -190.0, 180.0), LandUse::Residential);
    w.add_zone(square(10.0, 10.0, 180.0), LandUse::Commercial);
    w.add_zone(square(-190.0, 10.0, 180.0), LandUse::Industrial);
    w.add_zone(square(10.0, -190.0, 180.0), LandUse::Industrial);
    let mut minds = IdleMind;
    for _ in 0..(5 * 24 * 60) {
        w.tick(&mut minds);
    }
    let m = &w.economy.market;
    assert!(m.businesses > 3, "businesses should exist: {}", m.businesses);
    let groceries_in_town: f32 = w.buildings.values().filter_map(|b| b.business.as_ref()).map(|bz| bz.stock_of(Resource::Groceries)).sum();
    assert!(groceries_in_town > 0.0, "grocers should have stock from local or imported inputs");
    let deals: u32 = w.buildings.values().filter_map(|b| b.business.as_ref()).map(|bz| bz.deals).sum();
    assert!(deals > 0 || m.imports_today > 0.0, "either local deals or imports must have happened");
    // Save round-trip keeps businesses and cargo.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.economy.market.businesses, m.businesses);
}

#[test]
fn citizens_drive_between_buildings() {
    let mut w = World::new(WorldConfig::test_town());
    for i in -1..=1 {
        let c = i as f32 * 200.0;
        w.add_road(&[Vec2::new(-500.0, c), Vec2::new(500.0, c)], 1, 1);
        w.add_road(&[Vec2::new(c, -500.0), Vec2::new(c, 500.0)], 1, 1);
    }
    w.add_zone(square(-190.0, -190.0, 180.0), LandUse::Residential);
    w.add_zone(square(10.0, 10.0, 180.0), LandUse::Commercial);
    let mut minds = IdleMind;
    let mut saw_car = false;
    for _ in 0..(2 * 24 * 60) {
        w.tick(&mut minds);
        if !w.traffic.cars.is_empty() {
            saw_car = true;
        }
    }
    assert!(saw_car, "commuters should have driven at some point");
}

#[test]
fn bike_owners_cycle_long_trips_and_motion_is_tracked() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    let mut minds = IdleMind;
    for _ in 0..(6 * 60) {
        w.tick(&mut minds);
    }
    finish_all(&mut w);
    for _ in 0..60 {
        w.tick(&mut minds);
    }
    let c = w.citizens.keys().next().expect("someone moved in");
    {
        let cz = &mut w.citizens[c];
        cz.owns_car = false;
        cz.owns_bike = true;
        cz.inside = None;
        cz.activity = Activity::Idle;
        cz.pos = Vec2::new(-150.0, 10.0);
    }
    let far = w.buildings.iter().map(|(id, b)| (id, w.lots[b.lot].center)).max_by(|a, b| a.1.x.total_cmp(&b.1.x)).unwrap().0;
    w.travel_to(c, Some(far), Purpose::Work);
    assert_eq!(w.citizens[c].mode, TravelMode::Bike);
    assert!(matches!(w.citizens[c].activity, Activity::Traveling { .. }));
    let before = w.citizens[c].pos;
    w.tick(&mut minds);
    let cz = &w.citizens[c];
    assert!((cz.prev_pos.distance(before)) < 0.01, "previous pose is kept for interpolation");
    let moved = cz.pos.distance(before);
    assert!((moved - w.config.bike_speed).abs() < 0.5, "cycled {moved} m in a tick");
}

#[test]
fn a_turning_cyclist_keeps_to_the_kerb_and_never_crosses_the_lanes() {
    // An L of street, ridden both ways: a turn one way, then the other.
    for flip in [1.0, -1.0] {
        let mut w = World::new(WorldConfig::test_town());
        let pts = [Vec2::new(-150.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(0.0, 150.0 * flip)];
        w.add_road(&pts, 1, 1);
        let near = |p: Vec2| (0..w.roads.nodes.len() as u32).map(NodeId).min_by(|a, b| w.roads.node(*a).pos.distance(p).total_cmp(&w.roads.node(*b).pos.distance(p))).unwrap();
        for (from, to) in [(pts[0], pts[2]), (pts[2], pts[0])] {
            let nodes = w.roads.path(near(from), near(to)).expect("the L is one street");
            let path = w.route_path(&nodes, TravelMode::Bike);
            let half = w.roads.alive_segments().next().unwrap().1.half_width();
            let kerb_line = half - crate::roads::SHOULDER * 0.5;
            let centre = |p: Vec2| pts.windows(2).map(|s| {
                let (a, b) = (s[0], s[1]);
                let t = ((p - a).dot(b - a) / (b - a).length_squared()).clamp(0.0, 1.0);
                p.distance(a + (b - a) * t)
            }).fold(f32::MAX, f32::min);
            for p in &path {
                assert!(centre(*p) > kerb_line - 0.2, "flip {flip}: the rider's path passes {p} only {:.2} m off the centre line (kerb line {kerb_line:.2})", centre(*p));
            }
        }
    }
}

#[test]
fn park_visits_end_on_a_bench_and_people_leave_on_foot() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 120.0), LandUse::Park);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
    let park_lot = w.lots.iter().find(|(_, l)| l.usable() && l.land_use == LandUse::Park).map(|(id, _)| id).expect("a park lot");
    assert!(w.lots[park_lot].park.is_some(), "the park is planned");
    assert!(!w.walk.benches.is_empty(), "benches are in the walk graph");
    let home_lot = w.lots.iter().find(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, _)| id).unwrap();
    w.build_on(home_lot);
    w.build_on(park_lot);
    let park = w.lots[park_lot].building.expect("park building");
    let home = w.lots[home_lot].building.unwrap();
    w.finish_construction(home);
    w.finish_construction(park);
    let mut minds = IdleMind;
    for _ in 0..(8 * 60) {
        w.tick(&mut minds);
    }
    let c = w.citizens.keys().next().expect("someone arrived");
    w.citizens[c].inside = Some(home);
    w.citizens[c].activity = Activity::AtHome;
    w.citizens[c].owns_bike = false;
    w.travel_to(c, Some(park), Purpose::Visit(LandUse::Park));
    let mut sat = false;
    for _ in 0..1200 {
        w.tick(&mut minds);
        if matches!(w.citizens[c].activity, Activity::Sitting { .. }) {
            sat = true;
            break;
        }
    }
    assert!(sat, "the visitor ends up on a bench: {:?}", w.citizens[c].activity.label());
    let at = w.citizens[c].pos;
    assert!(w.lots[park_lot].park.as_ref().unwrap().benches.iter().any(|(b, _)| b.distance(at) < 0.95), "sitting on a planned bench (a seat along it)");
    assert!(w.citizens[c].is_visible() && w.citizens[c].inside.is_none());
    assert_eq!(w.buildings[park].visitors, 1);
    // Leaving: on foot from the bench, no jump.
    w.travel_to(c, Some(home), Purpose::Home);
    assert_eq!(w.buildings[park].visitors, 0);
    assert!(w.citizens[c].pos.distance(at) < 0.5, "starts from the bench");
    assert_eq!(w.citizens[c].mode, TravelMode::Walk);
    assert!(matches!(w.citizens[c].activity, Activity::Traveling { .. }));
}

#[test]
fn buses_need_a_depot_run_a_loop_and_carry_riders() {
    use crate::transit;
    let mut w = World::new(WorldConfig::test_town());
    // A block: two east-west streets joined by two north-south ones.
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-400.0, 140.0), Vec2::new(400.0, 140.0)], 1, 1);
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(-300.0, 140.0)], 1, 1);
    w.add_road(&[Vec2::new(300.0, 0.0), Vec2::new(300.0, 140.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-390.0, 10.0), Vec2::new(-310.0, 10.0), Vec2::new(-310.0, 130.0), Vec2::new(-390.0, 130.0)], LandUse::Residential);
    w.add_zone(vec![Vec2::new(310.0, 10.0), Vec2::new(390.0, 10.0), Vec2::new(390.0, 130.0), Vec2::new(310.0, 130.0)], LandUse::Commercial);
    w.add_zone(vec![Vec2::new(-100.0, -150.0), Vec2::new(100.0, -150.0), Vec2::new(100.0, -10.0), Vec2::new(-100.0, -10.0)], LandUse::Industrial);
    let route = w.add_bus_route();
    // Clockwise round the block, stops on the kerb the bus drives along.
    for p in [Vec2::new(-200.0, 4.0), Vec2::new(200.0, 4.0), Vec2::new(296.0, 70.0), Vec2::new(200.0, 136.0), Vec2::new(-200.0, 136.0), Vec2::new(-296.0, 70.0)] {
        w.add_bus_stop(route, p).expect("stop snaps to the street");
    }
    assert!(w.add_bus_stop(route, Vec2::new(-200.0, 4.0)).is_err(), "no duplicate stops");
    assert!(w.add_bus_stop(route, Vec2::new(0.0, 500.0)).is_err(), "far from any street");
    assert_eq!(w.transit.routes[route].stops.len(), 6);
    assert!(w.transit.routes[route].runnable(), "every leg has a road: {:?}", w.transit.routes[route].broken);
    assert!(w.transit.routes[route].legs.iter().all(|l| l.len() >= 2));
    let mut minds = IdleMind;
    // No depot: no bus, however long we wait.
    for _ in 0..200 {
        w.tick(&mut minds);
    }
    assert_eq!(w.traffic.cars.values().filter(|c| c.bus.is_some()).count(), 0, "buses need a depot");
    let depot_lot = w.lots.iter().find(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Industrial && l.area() >= transit::DEPOT_MIN_AREA).map(|(id, _)| id).expect("industrial lot");
    let home_lot = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential).min_by(|a, b| a.1.center.distance(Vec2::new(-320.0, 70.0)).total_cmp(&b.1.center.distance(Vec2::new(-320.0, 70.0)))).map(|(id, _)| id).unwrap();
    assert!(w.build_depot(home_lot).is_err(), "no depots on homes");
    let depot = w.build_depot(depot_lot).expect("depot built");
    assert!(w.buildings[depot].depot && w.economy.treasury < 60_000.0);
    let mut bus_seen = false;
    let mut stops_passed = 0usize;
    let mut last_next = 0usize;
    for _ in 0..1200 {
        w.tick(&mut minds);
        if let Some(car) = w.traffic.cars.values().find(|c| c.bus.is_some()) {
            bus_seen = true;
            let b = car.bus.as_ref().unwrap();
            if b.next_stop != last_next {
                stops_passed += 1;
                last_next = b.next_stop;
            }
        }
    }
    assert!(bus_seen, "a bus left the depot");
    assert!(stops_passed >= 4, "the bus went round the loop: {stops_passed} stops");
    assert!(w.economy.transit.buses >= 1);
    // A rider: lives in the west, works in the east, no bike, no car.
    w.build_on(home_lot);
    let shop_lot = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Commercial).min_by(|a, b| a.1.center.distance(Vec2::new(320.0, 70.0)).total_cmp(&b.1.center.distance(Vec2::new(320.0, 70.0)))).map(|(id, _)| id).unwrap();
    w.build_on(shop_lot);
    let home = w.lots[home_lot].building.unwrap();
    let shop = w.lots[shop_lot].building.unwrap();
    w.finish_construction(home);
    w.finish_construction(shop);
    for _ in 0..(6 * 60) {
        w.tick(&mut minds);
    }
    let c = w.citizens.keys().next().expect("someone arrived");
    // Buses sleep at the depot overnight: start the trip mid-morning
    // with a bus out on the line. A lap of the block takes longer than
    // a rider will wait, so set off as the bus heads for the stop
    // before the rider's.
    let bus_due = |w: &World| w.traffic.cars.values().any(|c| c.bus.as_ref().is_some_and(|b| b.next_stop == 4 && !b.to_depot));
    while !(7 * 60..=12 * 60).contains(&(w.clock.minute_of_day() as u32)) || w.economy.transit.buses == 0 || !bus_due(&w) {
        w.tick(&mut minds);
    }
    {
        let cz = &mut w.citizens[c];
        cz.inside = Some(home);
        cz.activity = Activity::AtHome;
        cz.owns_bike = false;
        cz.owns_car = false;
        cz.savings = 50.0;
    }
    w.travel_to(c, Some(shop), Purpose::Shop(Resource::Goods));
    assert!(matches!(w.citizens[c].activity, Activity::Traveling { purpose: Purpose::ToStop, .. }), "walks to the stop first: {}", w.citizens[c].activity.label());
    assert!(w.citizens[c].transit.is_some());
    let (mut waited, mut rode) = (false, false);
    let mut arrived = false;
    for _ in 0..3000 {
        w.tick(&mut minds);
        match w.citizens[c].activity {
            Activity::Waiting { .. } => waited = true,
            Activity::Riding { .. } => rode = true,
            _ => {}
        }
        if w.citizens[c].inside == Some(shop) {
            arrived = true;
            break;
        }
    }
    assert!(waited && rode && arrived, "waited {waited} rode {rode} arrived {arrived}: {}", w.citizens[c].activity.label());
    assert!(w.citizens[c].savings < 50.0, "paid the fare");
    assert!(w.transit.routes[route].riders_today >= 1 && w.economy.transit.fares_today > 0.0);
    // [lines] Every boarding is counted at its stop, the rider's alighting
    // too, and the buses have timed legs of the loop.
    let r = &w.transit.routes[route];
    assert_eq!(r.stops.iter().map(|s| s.stats.on_today).sum::<u32>(), r.riders_today, "boardings by stop add up to the line's riders");
    assert!(r.stops.iter().map(|s| s.stats.off_today).sum::<u32>() >= 1, "the rider got off somewhere");
    assert!(r.stops.iter().any(|s| s.stats.leg_runs > 0 && s.stats.leg_minutes > 0.0), "a leg was timed");
    for i in 0..r.stops.len() {
        assert!(r.leg_minutes(i, w.config.traffic_seconds_per_tick).0 > 0.0);
    }
    // Editing: moving and removing stops keeps the line sane; removing the
    // last stop removes the line and its buses.
    assert!(w.move_bus_stop(route, 1, Vec2::new(250.0, 4.0)));
    assert!(w.transit.routes[route].runnable());
    assert_eq!(w.transit.routes[route].stops[1].stats.leg_runs, 0, "a moved stop's leg is timed afresh");
    let fourth = w.transit.routes[route].stops[4].stats;
    w.remove_bus_stop(route, 3);
    assert_eq!(w.transit.routes[route].stops.len(), 5);
    let moved = w.transit.routes[route].stops[3].stats;
    assert_eq!((moved.on_today, moved.off_today), (fourth.on_today, fourth.off_today), "a stop's counts follow it when one before it goes");
    assert_eq!(w.transit.routes[route].stops[2].stats.leg_runs, 0, "the stop before the removed one now drives a new leg");
    assert!(w.rename_bus_route(route, "  ").is_err());
    w.rename_bus_route(route, "Harbour Loop").unwrap();
    w.recolour_bus_route(route, 3).unwrap();
    assert_eq!((w.transit.routes[route].name.as_str(), w.transit.routes[route].color), ("Harbour Loop", 3));
    assert!(w.traffic.cars.values().filter(|c| c.bus.as_ref().is_some_and(|b| b.route == route)).all(|c| c.model.paint == 3), "buses on the road take the new colour");
    w.set_route_buses(route, 2);
    assert_eq!(w.transit.routes[route].buses, 2);
    w.remove_bus_route(route);
    assert!(w.transit.routes.is_empty());
    // [services] Its buses let their riders off and drive home to the
    // depot (they used to vanish where they stood), then are gone.
    let out: Vec<CarId> = w.traffic.cars.iter().filter(|(_, c)| c.bus.is_some()).map(|(id, _)| id).collect();
    for &b in &out {
        let bus = w.traffic.cars[b].bus.as_ref().unwrap();
        assert!(bus.passengers.is_empty() && bus.to_depot && bus.next_stop == usize::MAX && bus.depot == depot, "heading home empty");
    }
    let depot_at = w.building_lot(depot).unwrap().car_point;
    let mut last: std::collections::HashMap<CarId, Vec2> = std::collections::HashMap::new();
    for _ in 0..600 {
        for &b in &out {
            if let Some(c) = w.traffic.cars.get(b) {
                last.insert(b, c.pos);
            }
        }
        w.tick(&mut minds);
        if w.traffic.cars.values().all(|c| c.bus.is_none()) {
            break;
        }
    }
    assert_eq!(w.traffic.cars.values().filter(|c| c.bus.is_some()).count(), 0);
    for (b, p) in &last {
        assert!(p.distance(depot_at) < 40.0, "bus {b:?} left the road at its depot, not at {p}");
    }
    // Round trip keeps the depot.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert!(back.buildings[depot].depot);
}

#[test]
fn trains_call_at_stations_and_yards_bring_freight() {
    use crate::rail::{Facility, RailFacility, TrainKind};
    let mut w = World::new(WorldConfig::test_town());
    assert!(w.roads.alive_segments().any(|(_, s)| s.is_rail()), "the trunk line is there from the start");
    w.economy.treasury = 200_000.0;
    // A branch off the trunk, curving north then running east, and a
    // street beside it. The drawn corner is square; the built track is not.
    w.add_rail(&[Vec2::new(-200.0, TRUNK_Y), Vec2::new(-100.0, -400.0), Vec2::new(-40.0, -300.0), Vec2::new(0.0, -200.0), Vec2::new(0.0, -60.0), Vec2::new(900.0, -60.0)]);
    assert!(w.roads.alive_segments().filter(|(_, s)| s.is_rail()).count() >= 4, "the branch split the trunk and joined it");
    {
        let branch: Vec<Vec2> = w.master.roads().filter(|(_, _, _, _, k)| *k == RoadKind::Rail).map(|(_, pts, ..)| pts).find(|p| p.len() > 2).unwrap();
        let built = crate::rail::smooth_track(&branch, &w.roads);
        assert!(crate::rail::sharpest_turn(&built) < 0.6, "no sharp bends on the built track: {:.2} rad", crate::rail::sharpest_turn(&built));
        let first_leg = (built[1] - built[0]).normalize_or_zero();
        assert!(first_leg.y.abs() < 0.35, "the branch leaves the trunk along it, not square off it: {first_leg:?}");
    }
    w.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(1000.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(120.0, 0.0), Vec2::new(120.0, 120.0)], 1, 1);
    w.add_road(&[Vec2::new(600.0, 0.0), Vec2::new(600.0, 120.0)], 1, 1);
    w.add_zone(vec![Vec2::new(150.0, 10.0), Vec2::new(330.0, 10.0), Vec2::new(330.0, 90.0), Vec2::new(150.0, 90.0)], LandUse::Residential);
    w.add_zone(vec![Vec2::new(650.0, 10.0), Vec2::new(830.0, 10.0), Vec2::new(830.0, 90.0), Vec2::new(650.0, 90.0)], LandUse::Commercial);
    w.add_zone(vec![Vec2::new(350.0, 10.0), Vec2::new(570.0, 10.0), Vec2::new(570.0, 130.0), Vec2::new(350.0, 130.0)], LandUse::Industrial);
    // A station set down beside the branch fuses to it; its street stub
    // is 26 m short of the main street, so it is not connected yet.
    let (plan, rail_fused, road_fused) = Facility::plan(RailFacility::Station, Vec2::new(240.0, -45.0), &w.roads);
    assert!(rail_fused && !road_fused, "fuses to the track only: rail {rail_fused} road {road_fused}");
    assert!((plan.center.y - (-40.0)).abs() < 0.5 && plan.along.x.abs() > 0.99, "sits along the track with its rail side on it: {:?} {:?}", plan.center, plan.along);
    let station_a = w.place_rail_facility(RailFacility::Station, Vec2::new(240.0, -45.0)).expect("station A");
    let la = w.buildings[station_a].rail.clone().unwrap();
    assert!(la.rail_connected && !la.road_connected, "track yes, road not yet: {la:?}");
    assert!(w.lots.get(w.buildings[station_a].lot).is_some_and(|l| l.front_dir.y > 0.9), "the lot fronts its street stub to the north");
    assert!(w.roads.alive_segments().any(|(_, s)| s.is_street() && w.roads.node(s.a).pos.y > -20.0 && w.roads.node(s.a).pos.y < -17.0), "a street stub was laid along the road side");
    // Joining the stub to the main street connects it.
    let stub_end = w.facilities.values().next().unwrap().road_line()[1];
    w.add_road(&[stub_end, Vec2::new(stub_end.x, 0.0)], 1, 1);
    let la = w.buildings[station_a].rail.clone().unwrap();
    assert!(la.works(), "connected once the stub meets the street: {la:?}");
    // Station B the same way, and a freight yard; both get their roads.
    let station_b = w.place_rail_facility(RailFacility::Station, Vec2::new(740.0, -45.0)).expect("station B");
    let end_b = w.facilities.values().find(|f| f.building == Some(station_b)).unwrap().road_line()[1];
    w.add_road(&[end_b, Vec2::new(end_b.x, 0.0)], 1, 1);
    assert!(w.place_rail_facility(RailFacility::Station, Vec2::new(760.0, -45.0)).is_err(), "too close to station B");
    let yard = w.place_rail_facility(RailFacility::FreightYard, Vec2::new(460.0, -50.0)).expect("yard");
    let end_y = w.facilities.values().find(|f| f.building == Some(yard)).unwrap().road_line()[1];
    w.add_road(&[end_y, Vec2::new(end_y.x, 0.0)], 1, 1);
    assert!(w.buildings[yard].rail.as_ref().unwrap().works());
    assert!(w.buildings[station_b].rail.as_ref().unwrap().works());
    // A free-standing yard far from everything lays both stubs and runs nothing.
    let lone = w.place_rail_facility(RailFacility::FreightYard, Vec2::new(-600.0, 300.0)).expect("lone yard");
    let ll = w.buildings[lone].rail.as_ref().unwrap();
    assert!(!ll.rail_connected && !ll.road_connected, "a yard in a field waits for track and road");
    assert!(w.facilities.values().filter(|f| f.lot.is_some()).count() == 4);
    // A rider from a home by station A to a shop by station B.
    let lot_near = |w: &World, p: Vec2, lu: LandUse| w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == lu).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|(id, _)| id).unwrap();
    let home_lot = lot_near(&w, Vec2::new(240.0, 40.0), LandUse::Residential);
    w.build_on(home_lot);
    let shop_lot = lot_near(&w, Vec2::new(815.0, 40.0), LandUse::Commercial);
    w.build_on(shop_lot);
    let home = w.lots[home_lot].building.unwrap();
    let shop = w.lots[shop_lot].building.unwrap();
    w.finish_construction(home);
    w.finish_construction(shop);
    // A passenger train comes within the headway and calls at both stations.
    let mut minds = IdleMind;
    let mut saw_train = false;
    let mut dwelt_at_a = false;
    for _ in 0..600 {
        w.tick(&mut minds);
        for t in w.rail.trains.values() {
            if t.kind == TrainKind::Passenger {
                saw_train = true;
                assert_eq!(t.stops.len(), 2, "both connected stations are on the route, the lone yard is not");
                if t.dwell > 0.0 && t.stops[t.next_stop.min(1)].0 == station_a {
                    dwelt_at_a = true;
                }
            }
        }
    }
    assert!(saw_train && dwelt_at_a, "train {saw_train} dwelt {dwelt_at_a}");
    assert!(w.rail.stats.passenger_trains_today >= 1);
    for _ in 0..(4 * 60) {
        w.tick(&mut minds);
    }
    while !(7 * 60..=14 * 60).contains(&(w.clock.minute_of_day() as u32)) {
        w.tick(&mut minds);
    }
    let c = w.citizens.keys().next().expect("someone arrived");
    {
        let cz = &mut w.citizens[c];
        cz.inside = Some(home);
        cz.activity = Activity::AtHome;
        cz.owns_bike = false;
        cz.owns_car = false;
        cz.savings = 50.0;
    }
    // Walking and the train cost about the same here; the day's whim in
    // mode choice decides, so ask again until the train wins.
    for _ in 0..20 {
        w.travel_to(c, Some(shop), Purpose::Shop(Resource::Goods));
        if w.citizens[c].transit.is_some() {
            break;
        }
        let home_e = w.building_entrance(home).unwrap();
        let cz = &mut w.citizens[c];
        cz.inside = Some(home);
        cz.activity = Activity::AtHome;
        cz.pos = home_e;
    }
    assert!(matches!(w.citizens[c].transit.map(|t| t.leg), Some(crate::transit::TransitLeg::Rail { board, alight }) if board == station_a && alight == station_b), "plans a rail leg: {:?}", w.citizens[c].transit);
    let (mut waited, mut rode, mut arrived) = (false, false, false);
    for _ in 0..1500 {
        w.tick(&mut minds);
        match w.citizens[c].activity {
            Activity::Waiting { .. } => waited = true,
            Activity::OnTrain { .. } => rode = true,
            _ => {}
        }
        if w.citizens[c].inside == Some(shop) {
            arrived = true;
            break;
        }
    }
    assert!(waited && rode && arrived, "waited {waited} rode {rode} arrived {arrived}: {}", w.citizens[c].activity.label());
    assert!(w.citizens[c].savings < 50.0 && w.rail.stats.riders_today >= 1);
    // Freight: an import goes to the connected yard, the buyer saves a
    // fifth, and the next freight train brings it in.
    let buyer = shop;
    assert!(w.buildings[buyer].business.is_some(), "the shop is a business that imports");
    let money0 = w.buildings[buyer].business.as_ref().unwrap().money;
    let queued0 = w.rail.orders_for(yard).0;
    w.import(buyer, Resource::Parts, 10.0, 100.0);
    assert_eq!(w.rail.orders_for(yard).0, queued0 + 1, "the order waits out of town for a train to the yard");
    assert_eq!(w.rail.orders_for(lone).0, 0, "the unconnected yard gets nothing");
    assert!((w.buildings[buyer].business.as_ref().unwrap().money - money0 - 20.0).abs() < 0.01, "rail import refunds a fifth");
    let mut freight_seen = false;
    // [train-inspect] [freight] While it is on its way the load rides the
    // train and the manifest lists it: for the shop, off at the yard; a
    // passenger train has none.
    let mut manifest_seen = false;
    let mut aboard_seen = false;
    for _ in 0..900 {
        w.tick(&mut minds);
        if let Some((tid, t)) = w.rail.trains.iter().find(|(_, t)| t.kind == TrainKind::Freight) {
            freight_seen = true;
            let aboard = t.cargo.iter().any(|c| c.to == buyer && c.from == yard);
            if aboard {
                aboard_seen = true;
                assert_eq!(w.rail.orders_for(yard).0, 0, "loaded at the edge of the map, not left waiting");
                let m = w.freight_manifest(tid).expect("a freight train has a manifest");
                assert!(m.aboard >= 10.0 && m.capacity >= m.aboard && m.wagons >= crate::rail::MIN_WAGONS, "{m:?}");
                manifest_seen |= m.loads.iter().any(|l| l.buyer == buyer && l.yard == yard && l.resource == Resource::Parts && (l.amount - 10.0).abs() < 0.01 && l.hub.is_none());
            }
        }
        if let Some((pid, _)) = w.rail.trains.iter().find(|(_, t)| t.kind == TrainKind::Passenger) {
            assert!(w.freight_manifest(pid).is_none());
        }
        if aboard_seen && w.rail.aboard_for(yard) == 0.0 {
            break;
        }
    }
    assert!(freight_seen && aboard_seen && w.rail.orders_for(yard).0 == 0 && w.rail.aboard_for(yard) == 0.0, "freight train {freight_seen}, aboard {aboard_seen}, waiting {:?}", w.rail.orders_for(yard));
    assert!(manifest_seen, "the freight train's manifest listed the shop's parts");
    if let Some((tid, _)) = w.rail.trains.iter().find(|(_, t)| t.kind == TrainKind::Freight) {
        let m = w.freight_manifest(tid).unwrap();
        assert!(m.unloaded >= 10.0 && m.loads.iter().all(|l| l.yard != yard), "set down at the yard: {m:?}");
    }
    assert!(w.rail.stats.freight_in_today >= 10.0);
    // Round trip keeps the facilities and their lots.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert!(back.buildings[station_a].rail.is_some() && back.buildings[yard].rail.is_some());
    assert_eq!(back.facilities.len(), 4);
}

#[test]
fn power_is_rationed_rich_first_and_plants_fix_it() {
    use crate::power::{demand_mw, PowerPlantKind};
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
    let ind: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial).map(|(id, _)| id).collect();
    let res: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, _)| id).collect();
    assert!(ind.len() >= 3 && res.len() >= 2);
    // Placement rules.
    assert!(w.build_power(res[0], PowerPlantKind::Coal).is_err(), "coal is industrial only");
    let big = ind.iter().copied().max_by(|a, b| w.lots[*a].area().total_cmp(&w.lots[*b].area())).unwrap();
    let treasury0 = w.economy.treasury;
    w.economy.treasury = 100.0;
    assert!(w.build_power(big, PowerPlantKind::Solar).is_err(), "no money");
    w.economy.treasury = treasury0;
    // Fill the rest with heavy industry: more demand than the regional grid sells.
    for &l in ind.iter().filter(|&&l| l != big) {
        w.build_on(l);
    }
    for &l in &res {
        w.build_on(l);
    }
    finish_all(&mut w);
    for b in w.buildings.values_mut() {
        b.floors = 30;
    }
    let demand: f32 = w.buildings.values().map(demand_mw).sum();
    assert!(demand > crate::power::GRID_IMPORT_MW, "demand {demand}");
    w.allocate_power();
    let p = &w.economy.power;
    assert!(p.served < 1.0 && p.unpowered_buildings > 0 && p.imported_mw > 0.0, "{p:?}");
    // The highest tiers are served before the lowest.
    let worst_high = w.buildings.values().filter(|b| b.tier == Tier::High).map(|b| b.powered).fold(1.0f32, f32::min);
    let best_low = w.buildings.values().filter(|b| b.tier == Tier::Low && demand_mw(b) > 0.0).map(|b| b.powered).fold(0.0f32, f32::max);
    if w.buildings.values().any(|b| b.tier == Tier::High) && w.buildings.values().any(|b| b.tier == Tier::Low) {
        assert!(worst_high >= best_low, "high {worst_high} vs low {best_low}");
    }
    // A nuclear plant covers everything.
    w.economy.treasury = 200_000.0;
    let plant = w.build_power(big, PowerPlantKind::Nuclear).expect("nuclear plant built");
    assert!(w.buildings[plant].plant == Some(PowerPlantKind::Nuclear) && w.buildings[plant].jobs == PowerPlantKind::Nuclear.jobs_at(w.plant_scale(&w.buildings[plant])));
    assert!(w.economy.treasury < 200_000.0);
    let p = &w.economy.power;
    assert_eq!(p.plants, 1);
    assert!(p.served >= 0.999 && p.unpowered_buildings == 0, "{p:?}");
    assert!(w.buildings.values().all(|b| b.powered >= 0.999));
    // Weather only matters to solar and wind.
    let noon = 0.5;
    let open = crate::weather::SiteResource::default();
    assert_eq!(PowerPlantKind::Nuclear.availability(&w.weather, noon, open), 1.0);
    let mut clear = w.weather.clone();
    clear.cloud = 0.0;
    let mut dull = clear.clone();
    dull.cloud = 1.0;
    assert!(PowerPlantKind::Solar.availability(&clear, noon, open) > PowerPlantKind::Solar.availability(&dull, noon, open));
    assert_eq!(PowerPlantKind::Solar.availability(&clear, 0.0, open), 0.0, "no sun at midnight");
    // Round trip keeps the plant.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.buildings[plant].plant, Some(PowerPlantKind::Nuclear));
}

/// The usable lot of `lu` nearest `p`, vacant.
fn lot_near(w: &World, p: Vec2, lu: LandUse) -> LotId {
    w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == lu).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|(id, _)| id).expect("a vacant lot")
}

#[test]
fn road_works_pull_down_the_buildings_whose_parcels_change() {
    let mut w = flat_world();
    let road = w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-400.0, 400.0), Vec2::new(400.0, 400.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.add_zone(square(10.0, 410.0, 190.0), LandUse::Residential);
    let front_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
    assert!(w.lots[front_lot].front_segment == w.roads.nearest_street(Vec2::new(100.0, 20.0)).unwrap().0, "the lot fronts the main road");
    w.build_on(front_lot);
    let fronting = w.lots[front_lot].building.unwrap();
    let far_lot = lot_near(&w, Vec2::new(100.0, 420.0), LandUse::Residential);
    w.build_on(far_lot);
    finish_all(&mut w);
    let far = w.lots[far_lot].building.unwrap();
    // People move in, so somebody is there to be displaced.
    let mut minds = IdleMind;
    for _ in 0..400 {
        w.tick(&mut minds);
    }
    assert!(!w.buildings[fronting].residents.is_empty(), "residents moved in");
    let resident = w.buildings[fronting].residents[0];
    let old_outline = w.lots[front_lot].polygon.clone();
    w.drain_events();

    // Widen the road to 3+3: every parcel along it loses a strip.
    w.set_gesture(road, Gesture::Road { points: vec![Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], lanes_fwd: 3, lanes_bwd: 3, curved: false, kind: RoadKind::Street, elevation: Vec::new() }, true);
    w.implement();
    // It comes down over the next day or so, on what is left of its parcel.
    assert!(w.buildings.get(fronting).is_some_and(|b| b.being_demolished()), "the building fronting the widened road is coming down");
    assert!(w.buildings.contains_key(far), "a building on an untouched lot stays");
    let events = w.drain_events();
    assert!(events.iter().any(|e| matches!(e, SimEvent::Expropriated(b, _) if *b == fronting)), "an expropriation event was raised: {events:?}");
    w.finish_demolition(fronting);
    assert!(!w.buildings.contains_key(fronting), "then it is gone");
    assert!(w.lots.values().all(|l| l.building != Some(fronting)));
    assert!(w.drain_events().iter().any(|e| matches!(e, SimEvent::BuildingRemoved(b, _) if *b == fronting)));
    let c = &w.citizens[resident];
    assert_eq!(c.home, None);
    assert!(c.memories.iter().any(|m| m.text.contains("pulled down for the road works")), "{:?}", c.memories);
    // The ground is still there, as a smaller lot, and can be built on again.
    let new_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
    assert!(outlines_differ(&old_outline, &w.lots[new_lot].polygon, EXPROPRIATE_AREA_TOL, EXPROPRIATE_DIST_TOL));

    // A footpath drawn through a built lot removes that building too.
    // (Lot ids change with every rebuild: find the building's lot afresh.)
    let far_center = w.lots[w.buildings[far].lot].center;
    w.add_path(&[Vec2::new(far_center.x, 380.0), Vec2::new(far_center.x, 620.0)]);
    assert!(w.buildings.get(far).is_none_or(|b| b.being_demolished()), "a path through the lot takes the building");
    if w.buildings.contains_key(far) {
        w.finish_demolition(far);
    }
    assert!(w.lots.values().all(|l| l.building != Some(far)));
}

#[test]
fn unchanged_parcels_keep_their_buildings_through_unrelated_works() {
    // A new road elsewhere must not shake buildings off parcels that did
    // not change: the outline comparison has to tolerate regeneration.
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
    for &l in &lots {
        w.build_on(l);
    }
    let n = w.buildings.len();
    assert!(n >= 3);
    w.add_road(&[Vec2::new(-300.0, -300.0), Vec2::new(-300.0, 300.0)], 2, 2);
    assert_eq!(w.buildings.len(), n, "nothing fronting the untouched road was expropriated");
    assert!(w.drain_events().iter().all(|e| !matches!(e, SimEvent::Expropriated(..))));
}

#[test]
fn stations_and_yards_never_share_ground_with_other_buildings() {
    use crate::rail::RailFacility;
    let mut w = World::new(WorldConfig::test_town());
    w.economy.treasury = 500_000.0;
    w.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(1000.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(150.0, 10.0), Vec2::new(330.0, 10.0), Vec2::new(330.0, 90.0), Vec2::new(150.0, 90.0)], LandUse::Commercial);
    w.add_zone(vec![Vec2::new(650.0, 10.0), Vec2::new(830.0, 10.0), Vec2::new(830.0, 90.0), Vec2::new(650.0, 90.0)], LandUse::Commercial);
    // A shop stands where the station would go: the city buys it out and
    // the station takes the ground.
    let shop_lot = lot_near(&w, Vec2::new(240.0, 25.0), LandUse::Commercial);
    w.build_on(shop_lot);
    let shop = w.lots[shop_lot].building.unwrap();
    let shop_center = w.lots[shop_lot].center;
    let station = w.place_rail_facility(RailFacility::Station, shop_center).expect("a station takes over a shop's ground");
    assert!(!w.buildings.contains_key(shop), "the shop was bought out");
    assert!(w.buildings.contains_key(station));
    // But one civic structure never replaces another.
    let err = w.place_rail_facility(RailFacility::FreightYard, shop_center).expect_err("a yard cannot replace a station");
    assert!(err.contains("bulldoze") || err.contains("too close"), "{err}");
    let lots_before = w.lots.len();
    // A yard over vacant zoned lots takes the ground; the lots under and
    // across its footprint give way.
    let yard = w.place_rail_facility(RailFacility::FreightYard, Vec2::new(740.0, 40.0)).expect("yard");
    assert!(w.lots.len() < lots_before, "zoned lots under the yard gave way");
    let fp = w.facilities.values().find(|f| f.building == Some(yard)).unwrap().footprint();
    let yard_lot = w.buildings[yard].lot;
    for (id, l) in w.lots.iter() {
        if id == yard_lot {
            continue;
        }
        assert!(!point_in_polygon(l.center, &fp), "a lot centre under the yard: {:?}", l.center);
        // As the rebuild judges it: a sliver along the edge where the
        // parcel cuts fall is not a crossing.
        assert!(!polygons_overlap(&plan::shrink_about_centroid(&fp, 0.995), &l.polygon), "a lot crossing the yard: {:?}", l.polygon);
    }
    // Nothing ever gets built on that ground: force development onto
    // every lot and let the city develop on its own for a while.
    let all: Vec<LotId> = w.lots.keys().collect();
    for l in all {
        w.build_on(l);
    }
    w.economy.demand = [1.0; 6];
    for _ in 0..50 {
        w.develop();
    }
    for (_, b) in w.buildings.iter() {
        // The station and the yard stand on their own facility ground.
        if b.rail.is_some() {
            continue;
        }
        let l = &w.lots[b.lot];
        assert!(!w.under_facility(l.center), "{} was built under the yard", b.name);
        assert!(!polygons_overlap(&fp, &l.polygon), "{} shares ground with the yard", b.name);
    }
    // Plants, depots and parking refuse facility ground too (the yard's
    // own lot is the only lot there, and it is taken).
    assert!(w.build_power(yard_lot, crate::power::PowerPlantKind::Solar).is_err());
    assert!(w.build_depot(yard_lot).is_err());
    assert!(w.build_parking(yard_lot).is_none());
    assert!(w.buildings.contains_key(yard) && w.buildings[yard].rail.is_some());
}

#[test]
fn cheap_buildings_on_dear_land_are_redeveloped_and_price_people_out() {
    // No random fires here: the test is about rent, not a blaze. (services)
    let mut w = flat_world();
    w.config.emergencies = false;
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.land_value.fill(0.1);
    let lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
    w.build_on(lot);
    finish_all(&mut w);
    let bid = w.lots[lot].building.unwrap();
    assert_eq!(w.buildings[bid].tier, Tier::Low);
    let rent0 = w.buildings[bid].rent;
    // Tenants move in while the land is cheap. The grid is recomputed
    // every two hours, so the scenario value is pinned before each tick.
    // Nobody here eats (IdleMind), so satiety is pinned: the test is
    // about rent driving people out, not hunger.
    let fed = |w: &mut World| {
        for c in w.citizens.values_mut() {
            c.satiety = 1.0;
        }
    };
    let mut minds = IdleMind;
    for _ in 0..600 {
        w.land_value.fill(0.1);
        fed(&mut w);
        w.tick(&mut minds);
    }
    let residents = w.buildings[bid].residents.clone();
    assert!(residents.len() >= 2, "tenants moved in: {}", residents.len());
    assert_eq!(w.buildings[bid].tier, Tier::Low, "cheap land keeps a cheap building");
    // The land becomes dear. Three days of that and the building is rebuilt.
    for _ in 0..(REDEVELOP_DAYS as usize + 1) * 1440 {
        w.land_value.fill(0.95);
        fed(&mut w);
        w.tick(&mut minds);
    }
    let b = &w.buildings[bid];
    assert!(b.tier > Tier::Low, "the building left the low tier: {:?}", b.tier);
    assert!(b.rent > rent0 * 2.0, "rent rose from {rent0:.0} to {:.0}", b.rent);
    assert!(b.residents.iter().all(|c| residents.contains(c)), "no one new moved in yet");
    assert!(w.citizens.values().any(|c| c.memories.iter().any(|m| m.text.contains("rebuilt for richer tenants"))));
    // Torn down, not refitted: the old tenants left when the wrecking crew
    // came, and the new building opens at the new rent.
    for cid in &residents {
        assert!(w.citizens.get(*cid).map_or(true, |c| c.home != Some(bid)), "an old tenant is still there");
    }
    for _ in 0..5 * 1440 {
        w.land_value.fill(0.95);
        fed(&mut w);
        w.tick(&mut minds);
    }
    assert!(w.citizens.values().all(|c| c.hungry_days == 0), "nobody is starving: what follows is about rent");
    let b = &w.buildings[bid];
    assert!(b.construction.is_none() && b.demolition.is_none(), "the new building opened");
    assert!(w.economy.displaced_total + w.economy.emigrated_total > 0);
}

#[test]
fn heights_follow_density() {
    let mut w = World::new(WorldConfig::test_town());
    // One city block ringed by streets.
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-400.0, 240.0), Vec2::new(400.0, 240.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 340.0)], 1, 1);
    w.add_road(&[Vec2::new(240.0, -100.0), Vec2::new(240.0, 340.0)], 1, 1);
    w.add_zone(square(0.0, 0.0, 240.0), LandUse::Residential);
    // Dear land: a tower zone, but nothing stands yet.
    w.land_value.fill(0.95);
    let middle = Vec2::new(120.0, 120.0);
    let mut lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
    lots.sort_by(|a, b| w.lots[*a].center.distance(middle).total_cmp(&w.lots[*b].center.distance(middle)));
    assert!(lots.len() >= 16, "{} lots", lots.len());
    assert_eq!(w.height_cap(lots[0]), 2, "an empty field allows two floors");
    w.build_on(lots[0]);
    let first = w.buildings[w.lots[lots[0]].building.unwrap()].clone();
    assert_eq!(first.tier, Tier::High);
    assert!(first.floors >= 1 && first.floors <= 3, "the first building is short: {} floors", first.floors);
    // A few more go up around it; they stay short while the block is sparse.
    for &l in &lots[1..6] {
        w.build_on(l);
    }
    assert!(w.buildings.values().all(|b| b.floors <= 4), "{:?}", w.buildings.values().map(|b| b.floors).collect::<Vec<_>>());
    // Once the block is dense (every neighbour a tower), the full range opens.
    for b in w.buildings.values_mut() {
        b.floors = 40;
    }
    assert!(w.height_cap(lots[6]) >= 12, "cap {}", w.height_cap(lots[6]));
    let mut tallest = 0;
    for &l in &lots[6..12] {
        w.build_on(l);
        tallest = tallest.max(w.buildings[w.lots[l].building.unwrap()].floors);
    }
    assert!(tallest > 6, "a new building in a dense block can be tall: {tallest}");
    // Parks are untouched by the rule.
    let mut p = World::new(WorldConfig::test_town());
    p.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    p.add_zone(square(10.0, 10.0, 100.0), LandUse::Park);
    let park = lot_near(&p, Vec2::new(60.0, 20.0), LandUse::Park);
    p.build_on(park);
    assert_eq!(p.buildings[p.lots[park].building.unwrap()].floors, 0);
}

#[test]
fn buildings_go_up_over_days_and_people_move_in_after() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    let lot = w.lots.iter().find(|(_, l)| l.usable()).map(|(id, _)| id).unwrap();
    w.build_on(lot);
    let id = w.lots[lot].building.unwrap();
    let (floors, tier) = (w.buildings[id].floors, w.buildings[id].tier);
    {
        let b = &w.buildings[id];
        let c = b.construction.as_ref().expect("a building site, not a building");
        assert_eq!(c.progress, 0.0);
        assert!((c.days - Construction::days_for(floors, tier)).abs() < 1e-6 && c.days >= 1.0);
        assert_eq!(b.homes, 0, "nobody can live on a site");
        assert_eq!(b.jobs, Construction::crew(floors), "the crew is the only work here");
        assert!(b.wage() > 0.0, "labourers are paid");
        assert_eq!(b.parking_capacity, 0);
    }
    let (planned_homes, _) = w.buildings[id].planned_capacity();
    assert!(planned_homes > 0);
    // The first load was ordered right away: a truck is on the road, or
    // it was dropped on the spot when no route exists.
    {
        let c = w.buildings[id].construction.as_ref().unwrap();
        assert!(c.deliveries == 1 || c.ordered, "{c:?}");
        assert!(w.economy.materials_today > 0.0, "parts are bought from outside when no fab is in town");
    }
    // The site is still standing after a save.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.buildings[id].construction, w.buildings[id].construction);

    let mut minds = IdleMind;
    let mut changed = 0usize;
    let mut hours = 0u32;
    w.drain_events();
    while w.buildings[id].construction.is_some() {
        for _ in 0..60 {
            w.tick(&mut minds);
        }
        hours += 1;
        changed += w.drain_events().iter().filter(|e| matches!(e, SimEvent::BuildingChanged(x) if *x == id)).count();
        let b = &w.buildings[id];
        if b.construction.is_some() {
            assert!(b.residents.is_empty() && b.homes == 0, "nobody moves in before the building opens");
        }
        assert!(w.economy.sites >= 1 || hours < 1);
        assert!(hours < 24 * 20, "construction never finished: {:?}", b.construction);
    }
    let days = Construction::days_for(floors, tier);
    assert!(hours as f32 >= days * 24.0 - 1.0, "finished in {hours} h, planned {days} days");
    assert!(hours as f32 <= days * 24.0 + 8.0, "took {hours} h for {days} days of work: stalled too long");
    assert!(changed >= 9, "the renderer saw the site grow: {changed} redraws");
    let b = &w.buildings[id];
    assert!(b.homes > 0 && b.homes == planned_homes, "homes {} planned {planned_homes}", b.homes);
    assert_eq!(b.jobs, 0);
    assert!(b.workers.is_empty(), "the crew was let go");
    // People move in once it is open.
    for _ in 0..240 {
        w.tick(&mut minds);
        if !w.buildings[id].residents.is_empty() {
            break;
        }
    }
    assert!(!w.buildings[id].residents.is_empty(), "residents move in after completion");
}

#[test]
fn a_site_waits_for_its_materials_and_resumes_on_delivery() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Commercial);
    let lot = w.lots.iter().find(|(_, l)| l.usable()).map(|(id, _)| id).unwrap();
    w.build_on(lot);
    let id = w.lots[lot].building.unwrap();
    let mut minds = IdleMind;
    // Let the first truck arrive, then pretend it never did: nothing happens.
    for _ in 0..600 {
        if w.buildings[id].construction.as_ref().unwrap().deliveries >= 1 {
            break;
        }
        w.tick(&mut minds);
    }
    let p0 = {
        let c = w.buildings[id].construction.as_mut().unwrap();
        assert_eq!(c.deliveries, 1, "the first load came: {c:?}");
        c.deliveries = 0;
        c.ordered = true;
        assert!(c.waiting_for_materials());
        c.progress
    };
    for _ in 0..180 {
        w.tick(&mut minds);
    }
    assert_eq!(w.buildings[id].construction.as_ref().unwrap().progress, p0, "no parts, no work");
    assert_eq!(w.buildings[id].construction.as_ref().unwrap().deliveries, 0, "no second order while one is on its way");
    // The truck arrives.
    w.deliver(Cargo { from: id, to: id, resource: Resource::Parts, amount: 8.0, price: 0.0 });
    {
        let c = w.buildings[id].construction.as_ref().unwrap();
        assert_eq!(c.deliveries, 1);
        assert!(!c.ordered && !c.waiting_for_materials());
    }
    for _ in 0..60 {
        w.tick(&mut minds);
    }
    let p = w.buildings[id].construction.as_ref().unwrap().progress;
    assert!(p > p0, "work resumed: {p}");
    // Work stops again at the quarter until the next load comes (the
    // one ordered ahead is still on the road).
    {
        let c = w.buildings[id].construction.as_mut().unwrap();
        c.progress = 0.25 - c.hourly_step() * 0.5;
        c.deliveries = 1;
        c.ordered = true;
    }
    for _ in 0..240 {
        w.tick(&mut minds);
    }
    let c = w.buildings[id].construction.as_ref().unwrap();
    assert!((c.progress - 0.25).abs() < 1e-6, "stalls at the quarter: {}", c.progress);
    assert!(c.waiting_for_materials());
    w.deliver(Cargo { from: id, to: id, resource: Resource::Parts, amount: 8.0, price: 0.0 });
    for _ in 0..60 {
        w.tick(&mut minds);
    }
    assert!(w.buildings[id].construction.as_ref().unwrap().progress > 0.25);
    // A delivery to a site never lands on the shop's shelves.
    let stock: f32 = w.buildings[id].business.as_ref().map_or(0.0, |bz| bz.stock_of(Resource::Parts));
    assert_eq!(stock, 0.0);
    // Skipping the wait opens the building with its real jobs.
    let planned = w.buildings[id].planned_capacity();
    w.finish_construction(id);
    let b = &w.buildings[id];
    assert!(b.construction.is_none());
    assert_eq!((b.homes, b.jobs), planned);
    assert!(b.jobs > 0 && b.jobs != Construction::crew(b.floors));
}

#[test]
fn site_crews_commute_and_are_released_when_the_building_opens() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    let home_lot = w.lots.iter().find(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, _)| id).unwrap();
    let site_lot = w.lots.iter().find(|(_, l)| l.usable() && l.land_use == LandUse::Commercial).map(|(id, _)| id).unwrap();
    w.build_on(home_lot);
    let home = w.lots[home_lot].building.unwrap();
    w.finish_construction(home);
    w.build_on(site_lot);
    let site = w.lots[site_lot].building.unwrap();
    // Make the site the only employer so the newcomers join the crew.
    let others: Vec<BuildingId> = w.buildings.keys().filter(|&b| b != home && b != site).collect();
    for b in others {
        w.buildings[b].jobs = 0;
    }
    let mut minds = IdleMind;
    for _ in 0..(4 * 60) {
        w.tick(&mut minds);
        for b in w.buildings.values_mut() {
            if b.construction.is_some() && b.lot != site_lot {
                b.jobs = 0;
            }
        }
    }
    assert!(!w.buildings[site].workers.is_empty(), "the unemployed take site work");
    let crew = w.buildings[site].workers.clone();
    for &c in &crew {
        assert_eq!(w.citizens[c].work, Some(site));
        assert!((w.citizens[c].income - Building::SITE_WAGE * (0.75 + (w.citizens[c].seed % 1000) as f32 / 1000.0 * 0.6)).abs() < 0.01);
    }
    w.finish_construction(site);
    assert!(w.buildings[site].workers.is_empty());
    for &c in &crew {
        assert_eq!(w.citizens[c].work, None, "laid off cleanly");
        assert_ne!(w.citizens[c].inside, Some(site));
    }
}

#[test]
fn player_placed_plants_and_depots_work_from_the_moment_of_placement() {
    use crate::power::PowerPlantKind;
    use crate::transit;
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
    let ind: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial).map(|(id, _)| id).collect();
    let big = ind.iter().copied().max_by(|a, b| w.lots[*a].area().total_cmp(&w.lots[*b].area())).unwrap();
    let plant = w.build_power(big, PowerPlantKind::Coal).expect("coal plant");
    let b = &w.buildings[plant];
    assert!(b.construction.is_none(), "specials are placed finished");
    // [plant-scale] Jobs follow the plant's size.
    assert_eq!(b.jobs, PowerPlantKind::Coal.jobs_at(b.plant_scale.unwrap()));
    assert_eq!(w.economy.power.plants, 1, "it is on the grid at once");
    let depot_lot = ind.iter().copied().find(|&l| l != big && w.lots[l].building.is_none() && w.lots[l].area() >= transit::DEPOT_MIN_AREA && w.lots[l].car_access());
    if let Some(l) = depot_lot {
        let depot = w.build_depot(l).expect("depot");
        assert!(w.buildings[depot].construction.is_none() && w.buildings[depot].jobs == transit::DEPOT_JOBS);
    }
}

#[test]
fn bulldozing_the_track_under_a_station_disconnects_it_without_panicking() {
    use crate::rail::RailFacility;
    let mut w = World::new(WorldConfig::test_town());
    w.economy.treasury = 200_000.0;
    let branch = w.add_rail(&[Vec2::new(-200.0, TRUNK_Y), Vec2::new(-100.0, -400.0), Vec2::new(-40.0, -300.0), Vec2::new(0.0, -200.0), Vec2::new(0.0, -60.0), Vec2::new(900.0, -60.0)]);
    w.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(1000.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(150.0, 10.0), Vec2::new(330.0, 10.0), Vec2::new(330.0, 90.0), Vec2::new(150.0, 90.0)], LandUse::Residential);
    let station = w.place_rail_facility(RailFacility::Station, Vec2::new(240.0, -45.0)).expect("station");
    assert!(w.buildings[station].rail.as_ref().unwrap().rail_connected);
    // Segment ids are renumbered by the rebuild; the old one must not be trusted.
    w.start_project();
    w.remove_gesture(branch);
    w.implement();
    let link = w.buildings.get(station).and_then(|b| b.rail.clone());
    assert!(link.as_ref().is_some_and(|l| !l.rail_connected), "the station stands but is off the railway: {link:?}");
    let mut minds = crate::mind::IdleMind;
    for _ in 0..600 {
        w.tick(&mut minds);
    }
    assert!(w.rail.trains.values().all(|t| t.stops.iter().all(|(s, _)| w.buildings.contains_key(*s))), "no train aims at a vanished stop");
}

// ── review follow-ups ────────────────────────────────────────────────────

/// The rail scenario of `trains_call_at_stations_and_yards_bring_freight`
/// up to the point where trains run: branch, street, zones, two
/// connected stations and a connected yard.
fn rail_city() -> (World, GestureId, BuildingId, BuildingId, BuildingId) {
    use crate::rail::RailFacility;
    let mut w = World::new(WorldConfig::test_town());
    w.economy.treasury = 200_000.0;
    let branch = w.add_rail(&[Vec2::new(-200.0, TRUNK_Y), Vec2::new(-100.0, -400.0), Vec2::new(-40.0, -300.0), Vec2::new(0.0, -200.0), Vec2::new(0.0, -60.0), Vec2::new(900.0, -60.0)]);
    w.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(1000.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(150.0, 10.0), Vec2::new(330.0, 10.0), Vec2::new(330.0, 90.0), Vec2::new(150.0, 90.0)], LandUse::Residential);
    w.add_zone(vec![Vec2::new(650.0, 10.0), Vec2::new(830.0, 10.0), Vec2::new(830.0, 90.0), Vec2::new(650.0, 90.0)], LandUse::Commercial);
    let place = |w: &mut World, kind, p| {
        let id = w.place_rail_facility(kind, p).expect("facility");
        let end = w.facilities.values().find(|f| f.building == Some(id)).unwrap().road_line()[1];
        w.add_road(&[end, Vec2::new(end.x, 0.0)], 1, 1);
        assert!(w.buildings[id].rail.as_ref().unwrap().works(), "{:?}", w.buildings[id].rail);
        id
    };
    let a = place(&mut w, RailFacility::Station, Vec2::new(240.0, -45.0));
    let b = place(&mut w, RailFacility::Station, Vec2::new(740.0, -45.0));
    let yard = place(&mut w, RailFacility::FreightYard, Vec2::new(460.0, -50.0));
    (w, branch, a, b, yard)
}

#[test]
fn bulldozing_the_branch_takes_the_trains_off_and_strands_riders_sanely() {
    use crate::rail::TrainKind;
    let (mut w, branch, station_a, station_b, _) = rail_city();
    let home_lot = lot_near(&w, Vec2::new(240.0, 40.0), LandUse::Residential);
    w.build_on(home_lot);
    let home = w.lots[home_lot].building.unwrap();
    w.finish_construction(home);
    let mut minds = IdleMind;
    for _ in 0..600 {
        if !w.citizens.is_empty() {
            break;
        }
        w.tick(&mut minds);
    }
    let rider = w.citizens.keys().next().expect("someone moved in");
    // Let a passenger train get well onto the branch.
    let mut train = None;
    for _ in 0..600 {
        w.tick(&mut minds);
        if let Some((id, t)) = w.rail.trains.iter().find(|(_, t)| t.kind == TrainKind::Passenger && t.on_branch()) {
            train = Some((id, t.head()));
            break;
        }
    }
    let (train, head) = train.expect("a train on the branch");
    // Someone is aboard, bound for station B.
    w.rail.trains[train].passengers.push(rider);
    w.citizens[rider].activity = Activity::OnTrain { train };
    w.citizens[rider].transit = Some(TransitTrip { leg: TransitLeg::Rail { board: station_a, alight: station_b }, target: home, purpose: Purpose::Home, since: 0 });
    w.start_project();
    w.remove_gesture(branch);
    w.implement();
    assert!(w.rail.trains.is_empty(), "every train came off with the rebuild");
    let c = &w.citizens[rider];
    assert!(matches!(c.activity, Activity::Idle) && c.transit.is_none(), "the rider stands where the train was: {}", c.activity.label());
    assert!(c.pos.distance(head) < 1.0 && c.pos.is_finite());
    assert!(!w.buildings[station_a].rail.as_ref().unwrap().rail_connected, "the stations are off the railway");
    for _ in 0..600 {
        w.tick(&mut minds);
        for t in w.rail.trains.values() {
            assert!(t.stops.iter().all(|(s, _)| w.buildings.contains_key(*s)), "no train aims at a vanished stop");
            for p in &t.poly {
                let (_, _, q) = w.roads.nearest_rail(*p).expect("track exists");
                assert!(q.distance(*p) < 3.0, "a train's route leaves the track at {p:?}");
            }
        }
    }
    assert!(w.citizens.values().all(|c| !matches!(c.activity, Activity::OnTrain { .. }) || w.rail.trains.values().any(|t| t.passengers.contains(&rider))));
}

#[test]
fn imports_stranded_at_a_disconnected_yard_go_by_truck_and_sites_never_wait() {
    let (mut w, branch, _, _, yard) = rail_city();
    let shop_lot = lot_near(&w, Vec2::new(815.0, 40.0), LandUse::Commercial);
    w.build_on(shop_lot);
    let shop = w.lots[shop_lot].building.unwrap();
    w.finish_construction(shop);
    // A site's parts never queue for a train.
    let queued0 = w.rail.orders_for(yard).0;
    let site_lot = lot_near(&w, Vec2::new(700.0, 40.0), LandUse::Commercial);
    w.build_on(site_lot);
    assert_eq!(w.rail.orders_for(yard).0, queued0, "construction parts came by truck");
    // A shop's order does, and it is stranded when the branch goes.
    w.import(shop, Resource::Parts, 10.0, 100.0);
    assert_eq!(w.rail.orders_for(yard).0, queued0 + 1);
    let stock0 = w.buildings[shop].business.as_ref().unwrap().stock_of(Resource::Parts);
    w.start_project();
    w.remove_gesture(branch);
    w.implement();
    assert!(!w.buildings[yard].rail.as_ref().unwrap().works());
    let mut minds = IdleMind;
    for _ in 0..60 {
        w.tick(&mut minds);
    }
    assert_eq!(w.rail.orders_for(yard).0, 0, "the stranded order came by road within the hour");
    let delivered = w.buildings[shop].business.as_ref().unwrap().stock_of(Resource::Parts) > stock0;
    let on_the_road = w.traffic.cars.values().any(|c| c.cargo.is_some_and(|k| k.to == shop && k.resource == Resource::Parts));
    assert!(delivered || on_the_road, "the parts are on a truck or on the shelf");
    // A working yard whose trains stopped coming is drained too.
    let (mut w, _, _, _, yard) = rail_city();
    let shop_lot = lot_near(&w, Vec2::new(815.0, 40.0), LandUse::Commercial);
    w.build_on(shop_lot);
    let shop = w.lots[shop_lot].building.unwrap();
    w.finish_construction(shop);
    while w.clock.tick % 60 != 59 {
        w.tick(&mut minds);
    }
    w.import(shop, Resource::Parts, 10.0, 100.0);
    assert_eq!(w.rail.orders_for(yard).0, 1);
    w.rail.last_freight_tick = 0;
    w.tick(&mut minds);
    assert_eq!(w.rail.orders_for(yard).0, 0, "two headways without a freight train and the goods go by road");
}

/// [freight] More goods than one train holds: each train is loaded to its
/// wagons' capacity at the edge of the map and no fuller, extra trains
/// follow for what was left, and everything is in town within hours, not
/// days. A save with goods aboard keeps them (back in the queue), and an
/// older save's orders waiting at the yard move to the queue.
#[test]
fn freight_trains_carry_what_fits_and_extra_trains_take_the_rest() {
    use crate::rail::{TrainKind, MAX_WAGONS, WAGON_CAPACITY};
    let (mut w, _, _, _, yard) = rail_city();
    let shop_lot = lot_near(&w, Vec2::new(815.0, 40.0), LandUse::Commercial);
    w.build_on(shop_lot);
    let shop = w.lots[shop_lot].building.unwrap();
    w.finish_construction(shop);
    let full = MAX_WAGONS as f32 * WAGON_CAPACITY;
    for _ in 0..16 {
        w.import(shop, Resource::Parts, 100.0, 500.0);
    }
    let ordered = w.rail.orders_for(yard).1;
    assert!(ordered > full * 2.0, "{ordered} units, more than two trains hold");
    let mut minds = IdleMind;
    let start = w.clock.tick;
    let mut trains_seen: Vec<crate::rail::TrainId> = Vec::new();
    let mut saved_mid_run = false;
    for _ in 0..900 {
        w.tick(&mut minds);
        for (id, t) in w.rail.trains.iter().filter(|(_, t)| t.kind == TrainKind::Freight) {
            assert!(t.freight_aboard() <= t.freight_capacity() + 0.01, "loaded past capacity: {} of {}", t.freight_aboard(), t.freight_capacity());
            if !trains_seen.contains(&id) {
                trains_seen.push(id);
            }
        }
        // Save while goods are aboard: none are lost.
        if !saved_mid_run && w.rail.aboard_for(yard) > 0.0 {
            saved_mid_run = true;
            let (queued, aboard) = (w.rail.orders_for(yard).1, w.rail.aboard_for(yard));
            let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
            assert!(back.rail.trains.is_empty());
            assert!((back.rail.orders_for(yard).1 - (queued + aboard)).abs() < 0.01, "goods aboard are saved back into the queue");
        }
        if w.rail.orders_for(yard).0 == 0 && w.rail.aboard_for(yard) == 0.0 {
            break;
        }
    }
    assert!(saved_mid_run, "a train carried goods");
    assert_eq!((w.rail.orders_for(yard).0, w.rail.aboard_for(yard)), (0, 0.0), "all of it came in within {} minutes", w.clock.tick - start);
    assert!(trains_seen.len() >= 3, "{} freight trains for {ordered} units", trains_seen.len());
    assert!(w.clock.tick - start < 600, "within ten hours: {}", w.clock.tick - start);
    // An older save: an order waiting at the yard itself.
    let mut old = w.clone();
    old.buildings[yard].rail_inbound.push(Cargo { from: yard, to: shop, resource: Resource::Parts, amount: 7.0, price: 5.0 });
    let text = old.to_ron().unwrap();
    assert!(text.contains("rail_inbound"));
    let back = World::from_ron(&text).unwrap();
    assert!(back.buildings[yard].rail_inbound.is_empty());
    assert_eq!(back.rail.orders_for(yard), (1, 7.0), "migrated to wait out of town for the next train");
}

#[test]
fn a_short_commute_at_eight_arrives_before_nine() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    let home_lot = lot_near(&w, Vec2::new(150.0, 20.0), LandUse::Residential);
    let work_lot = lot_near(&w, Vec2::new(-150.0, 20.0), LandUse::Commercial);
    w.build_on(home_lot);
    w.build_on(work_lot);
    let home = w.lots[home_lot].building.unwrap();
    let work = w.lots[work_lot].building.unwrap();
    w.finish_construction(home);
    w.finish_construction(work);
    let apart = w.lots[home_lot].entrance().distance(w.lots[work_lot].entrance());
    assert!((250.0..=350.0).contains(&apart), "home and work are {apart:.0} m apart");
    assert!((w.config.walk_speed - w.config.metres_per_tick(WALK_MPS)).abs() < 1e-6);
    assert!((w.config.bike_speed - BIKE_MPS * w.config.traffic_seconds_per_tick).abs() < 1e-6);
    let mut minds = IdleMind;
    for _ in 0..600 {
        if !w.citizens.is_empty() {
            break;
        }
        w.tick(&mut minds);
    }
    let c = w.citizens.keys().next().expect("someone moved in");
    {
        let cz = &mut w.citizens[c];
        cz.inside = Some(home);
        cz.activity = Activity::AtHome;
        cz.owns_bike = false;
        cz.owns_car = false;
        cz.work = Some(work);
    }
    w.clock.tick = (w.clock.day() + 1) * crate::clock::MINUTES_PER_DAY + 8 * 60;
    w.travel_to(c, Some(work), Purpose::Work);
    assert_eq!(w.citizens[c].mode, TravelMode::Walk);
    for _ in 0..120 {
        if w.citizens[c].inside == Some(work) {
            break;
        }
        w.tick(&mut minds);
    }
    assert_eq!(w.citizens[c].inside, Some(work), "arrived: {}", w.citizens[c].activity.label());
    assert!(w.clock.minute_of_day() < 9 * 60, "clocked in at {}", w.clock.format_hhmm());
}

#[test]
fn a_station_is_road_connected_only_through_a_zoned_island() {
    use crate::rail::RailFacility;
    let mut w = World::new(WorldConfig::test_town());
    w.economy.treasury = 500_000.0;
    // Island A: the town. Island B: a street with homes, not joined to
    // A. Island C: a street in a field.
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.add_road(&[Vec2::new(200.0, 600.0), Vec2::new(1000.0, 600.0)], 1, 1);
    w.add_zone(square(300.0, 610.0, 150.0), LandUse::Residential);
    w.add_road(&[Vec2::new(-900.0, 700.0), Vec2::new(-500.0, 700.0)], 1, 1);
    let on_b = w.place_rail_facility(RailFacility::Station, Vec2::new(700.0, 560.0)).expect("station on island B");
    let on_c = w.place_rail_facility(RailFacility::Station, Vec2::new(-700.0, 660.0)).expect("station on island C");
    let town_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
    let b_lot = w.buildings[on_b].lot;
    assert!(w.roads.path(w.lots[b_lot].access_node, w.lots[town_lot].access_node).is_none(), "islands A and B are separate");
    assert!(w.buildings[on_b].rail.as_ref().unwrap().road_connected, "a street with homes on it is a city, whichever lot the map lists first");
    assert!(!w.buildings[on_c].rail.as_ref().unwrap().road_connected, "a street in a field is not");
    // Joining C to A connects it.
    w.add_road(&[Vec2::new(-500.0, 700.0), Vec2::new(-400.0, 0.0)], 1, 1);
    assert!(w.buildings[on_c].rail.as_ref().unwrap().road_connected);
}

#[test]
fn a_save_with_dangling_ids_and_missing_fields_loads_cleanly() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    let mut minds = IdleMind;
    for _ in 0..(10 * 60) {
        w.tick(&mut minds);
    }
    finish_all(&mut w);
    for _ in 0..120 {
        w.tick(&mut minds);
    }
    let c = w.citizens.keys().next().expect("someone moved in");
    // An id nothing points at any more.
    let ghost = {
        let any = w.buildings.values().next().unwrap().clone();
        let id = w.buildings.insert(any);
        w.buildings.remove(id);
        id
    };
    {
        let cz = &mut w.citizens[c];
        cz.home = Some(ghost);
        cz.work = Some(ghost);
        cz.inside = Some(ghost);
        cz.parked_at = Some(ghost);
        cz.activity = Activity::Visiting(ghost);
        cz.transit = Some(TransitTrip { leg: TransitLeg::Rail { board: ghost, alight: ghost }, target: ghost, purpose: Purpose::Home, since: 0 });
    }
    let lot = w.lots.keys().next().unwrap();
    w.lots[lot].building = Some(ghost);
    let text = w.to_ron().unwrap();
    assert!(text.contains("version: 1"));
    // An older save: no version, and a config field that did not exist yet.
    let text = text.replacen("version: 1,", "", 1);
    let text = {
        let start = text.find("bike_threshold:").unwrap();
        let end = start + text[start..].find('\n').unwrap() + 1;
        format!("{}{}", &text[..start], &text[end..])
    };
    let mut back = World::from_ron(&text).expect("loads");
    assert_eq!(back.version, SAVE_VERSION);
    assert_eq!(back.config.bike_threshold, WorldConfig::default().bike_threshold, "a missing field takes its default");
    let cz = &back.citizens[c];
    assert!(cz.home.is_none() && cz.work.is_none() && cz.inside.is_none() && cz.parked_at.is_none() && cz.transit.is_none());
    assert!(matches!(cz.activity, Activity::Idle));
    assert!(back.lots[lot].building.is_none());
    for _ in 0..200 {
        back.tick(&mut minds);
    }
}

#[test]
fn the_overdraft_floor_refuses_purchases_and_doors_and_the_ledger_balances() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Commercial);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Entertainment);
    w.add_zone(square(-400.0, 10.0, 190.0), LandUse::Residential);
    let shop_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Commercial);
    let venue_lot = lot_near(&w, Vec2::new(-100.0, 20.0), LandUse::Entertainment);
    let home_lot = lot_near(&w, Vec2::new(-300.0, 20.0), LandUse::Residential);
    w.build_on(shop_lot);
    w.build_on(venue_lot);
    w.build_on(home_lot);
    let shop = w.lots[shop_lot].building.unwrap();
    let venue = w.lots[venue_lot].building.unwrap();
    finish_all(&mut w);
    w.buildings[shop].business = Some(Business::new(BusinessKind::GadgetShop, 4));
    w.buildings[shop].business.as_mut().unwrap().add(Resource::Goods, 5.0);
    assert_eq!(w.buildings[venue].business.as_ref().unwrap().kind, BusinessKind::Venue);
    let mut minds = IdleMind;
    for _ in 0..600 {
        if !w.citizens.is_empty() {
            break;
        }
        w.tick(&mut minds);
    }
    let c = w.citizens.keys().next().expect("someone moved in");
    // Skint: the shop sells nothing and the door is shut.
    w.citizens[c].savings = OVERDRAFT + 1.0;
    let mood0 = w.citizens[c].mood;
    w.buy(c, shop, Resource::Goods);
    assert_eq!(w.buildings[shop].business.as_ref().unwrap().stock_of(Resource::Goods), 5.0, "nothing sold");
    assert!(w.citizens[c].mood < mood0 && w.citizens[c].thought.contains("Skint"));
    let entrance = w.building_entrance(venue).unwrap();
    w.citizens[c].activity = Activity::Traveling { to: Some(venue), purpose: Purpose::Visit(LandUse::Entertainment), path: vec![entrance], idx: 1 };
    w.arrive(c);
    assert!(w.citizens[c].inside.is_none() && matches!(w.citizens[c].activity, Activity::Idle), "turned away");
    assert!(w.citizens[c].savings >= OVERDRAFT);
    // Flush: served and charged.
    w.citizens[c].savings = 100.0;
    w.citizens[c].activity = Activity::Traveling { to: Some(venue), purpose: Purpose::Visit(LandUse::Entertainment), path: vec![entrance], idx: 1 };
    w.arrive(c);
    assert_eq!(w.citizens[c].inside, Some(venue));
    assert!((w.citizens[c].savings - (100.0 - VENUE_COVER)).abs() < 0.01);
    w.citizens[c].inside = None;
    w.citizens[c].activity = Activity::Idle;
    w.buy(c, shop, Resource::Goods);
    assert_eq!(w.buildings[shop].business.as_ref().unwrap().stock_of(Resource::Goods), 4.0);
    // A shut venue is not somewhere to go out to.
    w.buildings[venue].business.as_mut().unwrap().closed = true;
    w.apply_decision(c, Decision { intent: Intent::Visit(LandUse::Entertainment), thought: String::new(), memory: None });
    assert!(matches!(w.citizens[c].activity, Activity::Idle), "nobody heads for a closed venue");
    // The ledger: every treasury move is booked, so the day's flows sum
    // to the day's change.
    let before = w.economy.treasury;
    let today0 = w.economy.ledger.today;
    // A home lot big enough for a solar farm (parcels are cut at a home's
    // size, so not every one is).
    let ind = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Residential && l.area() >= PowerPlantKind::Solar.min_area()).min_by(|a, b| a.1.center.distance(Vec2::new(-350.0, 20.0)).total_cmp(&b.1.center.distance(Vec2::new(-350.0, 20.0)))).map(|(id, _)| id).expect("a big enough home lot");
    w.lots[ind].land_use = LandUse::Industrial;
    w.build_power(ind, PowerPlantKind::Solar).expect("plant");
    assert!(w.economy.ledger.today_of(Flow::Construction) < 0.0);
    let booked: f32 = (0..9).map(|i| w.economy.ledger.today[i] - today0[i]).sum();
    assert!((w.economy.treasury - before - booked).abs() < 0.01, "treasury moved {} but the books say {}", w.economy.treasury - before, booked);
}

#[test]
fn same_seed_and_script_give_identical_saves() {
    fn run() -> String {
        let mut w = World::new(WorldConfig { seed: 11, ..WorldConfig::test_town() });
        for i in -1..=1 {
            let c = i as f32 * 200.0;
            w.add_road(&[Vec2::new(-500.0, c), Vec2::new(500.0, c)], 1, 1);
            w.add_road(&[Vec2::new(c, -500.0), Vec2::new(c, 500.0)], 1, 1);
        }
        w.add_zone(square(-190.0, -190.0, 180.0), LandUse::Residential);
        w.add_zone(square(10.0, 10.0, 180.0), LandUse::Commercial);
        w.add_zone(square(-190.0, 10.0, 180.0), LandUse::Industrial);
        w.add_zone(square(10.0, -190.0, 60.0), LandUse::Park);
        let mut minds = IdleMind;
        for _ in 0..(36 * 60) {
            w.tick(&mut minds);
        }
        finish_all(&mut w);
        for _ in 0..(6 * 60) {
            w.tick(&mut minds);
        }
        assert!(!w.citizens.is_empty() && !w.traffic.cars.is_empty(), "something happened");
        w.to_ron().unwrap()
    }
    let (a, b) = (run(), run());
    assert!(a == b, "two runs of the same script diverged");
}

/// A block with a bus line, a depot and a bus out on the loop.
fn bus_city() -> (World, RouteId) {
    use crate::transit;
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-400.0, 140.0), Vec2::new(400.0, 140.0)], 1, 1);
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(-300.0, 140.0)], 1, 1);
    w.add_road(&[Vec2::new(300.0, 0.0), Vec2::new(300.0, 140.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-390.0, 10.0), Vec2::new(-310.0, 10.0), Vec2::new(-310.0, 130.0), Vec2::new(-390.0, 130.0)], LandUse::Residential);
    w.add_zone(vec![Vec2::new(310.0, 10.0), Vec2::new(390.0, 10.0), Vec2::new(390.0, 130.0), Vec2::new(310.0, 130.0)], LandUse::Commercial);
    w.add_zone(vec![Vec2::new(-100.0, -150.0), Vec2::new(100.0, -150.0), Vec2::new(100.0, -10.0), Vec2::new(-100.0, -10.0)], LandUse::Industrial);
    let route = w.add_bus_route();
    for p in [Vec2::new(-200.0, 4.0), Vec2::new(200.0, 4.0), Vec2::new(296.0, 70.0), Vec2::new(200.0, 136.0), Vec2::new(-200.0, 136.0), Vec2::new(-296.0, 70.0)] {
        w.add_bus_stop(route, p).expect("stop snaps to the street");
    }
    let depot_lot = w.lots.iter().find(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Industrial && l.area() >= transit::DEPOT_MIN_AREA).map(|(id, _)| id).expect("industrial lot");
    w.build_depot(depot_lot).expect("depot built");
    (w, route)
}

#[test]
fn a_save_mid_journey_keeps_trucks_buses_and_riders() {
    let (mut w, route) = bus_city();
    let home_lot = lot_near(&w, Vec2::new(-350.0, 70.0), LandUse::Residential);
    let shop_lot = lot_near(&w, Vec2::new(350.0, 70.0), LandUse::Commercial);
    w.build_on(home_lot);
    w.build_on(shop_lot);
    finish_all(&mut w);
    let mut minds = IdleMind;
    // Run until a bus is out and people have moved in; sites order
    // trucks of their own as the city grows.
    for _ in 0..(12 * 60) {
        w.tick(&mut minds);
    }
    let bus = w.traffic.cars.iter().find(|(_, c)| c.bus.is_some()).map(|(id, _)| id).expect("a bus is out");
    let riders: Vec<CitizenId> = w.citizens.keys().take(3).collect();
    assert!(!riders.is_empty(), "someone moved in");
    for &r in &riders {
        w.traffic.cars[bus].bus.as_mut().unwrap().passengers.push(r);
        let c = &mut w.citizens[r];
        c.inside = None;
        c.activity = Activity::Riding { bus };
        c.transit = Some(TransitTrip { leg: TransitLeg::Bus { route, board: 0, alight: 3 }, target: w.lots[shop_lot].building.unwrap(), purpose: Purpose::Shop(Resource::Goods), since: 0 });
    }
    // A truck on the road too.
    let shop = w.lots[shop_lot].building.unwrap();
    w.import(shop, Resource::Goods, 3.0, 10.0);
    let trucks = w.traffic.cars.values().filter(|c| c.cargo.is_some()).count();
    let cars = w.traffic.cars.len();
    let riding = w.citizens.values().filter(|c| matches!(c.activity, Activity::Riding { .. })).count();
    assert!(trucks >= 1 && riding == riders.len(), "trucks {trucks} riding {riding}");
    let mut back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.traffic.cars.len(), cars, "every car reattached to its lane");
    assert_eq!(back.traffic.cars.values().filter(|c| c.cargo.is_some()).count(), trucks);
    assert_eq!(back.traffic.cars[bus].bus.as_ref().unwrap().passengers.len(), riders.len());
    assert_eq!(back.citizens.values().filter(|c| matches!(c.activity, Activity::Riding { .. })).count(), riding);
    for _ in 0..200 {
        back.tick(&mut minds);
    }
    assert!(back.citizens.values().all(|c| c.pos.is_finite()));
}

#[test]
fn a_business_that_cannot_pay_closes_goes_derelict_and_is_demolished() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Commercial);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
    let shop_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Commercial);
    let home_lot = lot_near(&w, Vec2::new(-100.0, 20.0), LandUse::Residential);
    w.build_on(shop_lot);
    w.build_on(home_lot);
    let shop = w.lots[shop_lot].building.unwrap();
    finish_all(&mut w);
    w.buildings[shop].business = Some(Business::new(BusinessKind::Bakery, w.buildings[shop].jobs));
    let mut minds = IdleMind;
    for _ in 0..600 {
        if !w.citizens.is_empty() {
            break;
        }
        w.tick(&mut minds);
    }
    let worker = w.citizens.keys().next().expect("someone moved in");
    if w.citizens[worker].work != Some(shop) {
        if let Some(old) = w.citizens[worker].work {
            w.buildings[old].workers.retain(|&c| c != worker);
        }
        w.citizens[worker].work = None;
        w.assign_job(worker, shop);
    }
    w.buildings[shop].business.as_mut().unwrap().money = -1000.0;
    w.drain_events();
    for day in 0..5 {
        assert!(!w.buildings[shop].business.as_ref().unwrap().closed, "still open on day {day}");
        w.daily_businesses();
    }
    let b = &w.buildings[shop];
    assert!(b.business.as_ref().unwrap().closed, "five broke days and it shuts");
    assert!(b.is_derelict(), "a shut shop is a derelict shell: condition {}", b.condition);
    assert!(b.workers.is_empty());
    assert_eq!(w.citizens[worker].work, None);
    assert_eq!(w.citizens[worker].income, GIG_INCOME);
    assert!(w.citizens[worker].memories.iter().any(|m| m.text.contains("shut down")));
    assert!(w.drain_events().iter().any(|e| matches!(e, SimEvent::BusinessClosed(x) if *x == shop)));
    assert_eq!(w.economy.market.closed_total, 1);
    for _ in 0..7 {
        w.daily_businesses();
        assert!(w.buildings.contains_key(shop), "a week of grace");
    }
    w.daily_businesses();
    assert!(w.buildings[shop].being_demolished(), "then it is pulled down");
    w.finish_demolition(shop);
    assert!(!w.buildings.contains_key(shop), "and the lot is cleared");
    assert!(w.lots[shop_lot].building.is_none() || !w.buildings.contains_key(w.lots[shop_lot].building.unwrap()));
}

#[test]
fn four_hungry_days_and_a_citizen_leaves() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    let home_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
    w.build_on(home_lot);
    let home = w.lots[home_lot].building.unwrap();
    w.finish_construction(home);
    let mut minds = IdleMind;
    for _ in 0..600 {
        if !w.citizens.is_empty() {
            break;
        }
        w.tick(&mut minds);
    }
    let c = w.citizens.keys().next().expect("someone moved in");
    assert!(w.buildings[home].residents.contains(&c));
    w.citizens[c].satiety = 0.0;
    w.citizens[c].savings = 1000.0;
    for day in 0..3 {
        w.daily_citizen(c);
        assert!(w.citizens.contains_key(c), "still here after {} hungry days", day + 1);
        assert_eq!(w.citizens[c].hungry_days, day + 1);
    }
    w.daily_citizen(c);
    assert!(!w.citizens.contains_key(c), "gone on the fourth");
    assert!(!w.buildings[home].residents.contains(&c));
    assert_eq!(w.economy.emigrated_total, 1);
    assert_eq!(w.economy.emigrated_by[crate::economy::LeaveReason::Hunger.index()], 1, "counted as leaving over hunger");
    assert!(w.drain_events().iter().any(|e| matches!(e, SimEvent::CitizenLeft(x) if *x == c)));
}

#[test]
fn job_search_fills_open_jobs_with_the_fixed_skill_formula() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    let home_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
    w.build_on(home_lot);
    w.finish_construction(w.lots[home_lot].building.unwrap());
    let mut minds = IdleMind;
    for _ in 0..600 {
        if !w.citizens.is_empty() {
            break;
        }
        w.tick(&mut minds);
    }
    let c = w.citizens.keys().next().expect("someone moved in");
    if let Some(old) = w.citizens[c].work {
        w.buildings[old].workers.retain(|&x| x != c);
    }
    w.lay_off(c, None);
    assert_eq!((w.citizens[c].work, w.citizens[c].income), (None, GIG_INCOME));
    // No jobs anywhere: nothing happens.
    for b in w.buildings.values_mut() {
        b.jobs = 0;
    }
    w.job_search();
    assert_eq!(w.citizens[c].work, None);
    // One opening, and it is filled at the wage times the person's skill.
    let shop_lot = lot_near(&w, Vec2::new(-100.0, 20.0), LandUse::Commercial);
    w.build_on(shop_lot);
    let shop = w.lots[shop_lot].building.unwrap();
    w.finish_construction(shop);
    w.buildings[shop].jobs = 1;
    w.job_search();
    assert_eq!(w.citizens[c].work, Some(shop));
    assert!(w.buildings[shop].workers.contains(&c));
    let expected = w.buildings[shop].wage() * World::skill_of(w.citizens[c].seed);
    assert!((w.citizens[c].income - expected).abs() < 0.01);
    assert_eq!(w.buildings[shop].free_jobs(), 0);
}

#[test]
fn gini_is_zero_when_equal_and_near_one_when_one_has_everything() {
    assert_eq!(Economy::gini(&mut vec![]), 0.0);
    assert_eq!(Economy::gini(&mut vec![50.0]), 0.0);
    assert!(Economy::gini(&mut vec![40.0, 40.0, 40.0, 40.0]).abs() < 1e-6);
    let g = Economy::gini(&mut vec![0.0, 0.0, 0.0, 100.0]);
    assert!((g - 0.75).abs() < 1e-6, "(n-1)/n for a single earner: {g}");
    let mut unsorted = vec![100.0, 0.0, 0.0, 0.0];
    assert!((Economy::gini(&mut unsorted) - 0.75).abs() < 1e-6, "order does not matter");
    assert!(Economy::gini(&mut vec![10.0, 20.0, 30.0, 40.0]) < g);
    assert_eq!(Economy::gini(&mut vec![0.0, 0.0]), 0.0, "no income, no inequality");
}

#[test]
fn removing_a_bus_stop_shifts_riders_and_buses_and_cancels_trips_through_it() {
    let (mut w, route) = bus_city();
    let home_lot = lot_near(&w, Vec2::new(-350.0, 70.0), LandUse::Residential);
    w.build_on(home_lot);
    finish_all(&mut w);
    let mut minds = IdleMind;
    for _ in 0..600 {
        if !w.citizens.is_empty() {
            break;
        }
        w.tick(&mut minds);
    }
    let shop = w.buildings.keys().next().unwrap();
    let ids: Vec<CitizenId> = w.citizens.keys().collect();
    let (a, b) = (ids[0], *ids.get(1).unwrap_or(&ids[0]));
    let trip = |board, alight| Some(TransitTrip { leg: TransitLeg::Bus { route, board, alight }, target: shop, purpose: Purpose::Shop(Resource::Goods), since: 0 });
    w.citizens[a].transit = trip(1, 4);
    w.citizens[a].activity = Activity::Waiting { at: Vec2::ZERO };
    if b != a {
        w.citizens[b].transit = trip(2, 5);
        w.citizens[b].activity = Activity::Waiting { at: Vec2::ZERO };
    }
    // A bus between stops 1 and 3, heading for 3.
    let stops = w.transit.routes[route].stops.clone();
    let (lanes, s0, s1) = w.traffic.net.find_route(stops[1].segment, stops[1].pos, stops[3].segment, stops[3].pos).expect("route");
    let car = w.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, 0);
    let depot = w.buildings.iter().find(|(_, b)| b.depot).map(|(id, _)| id).unwrap();
    w.traffic.cars[car].bus = Some(Bus { route, next_stop: 3, passengers: Vec::new(), to_depot: false, depot });
    w.remove_bus_stop(route, 2);
    assert_eq!(w.transit.routes[route].stops.len(), 5);
    assert_eq!(w.traffic.cars[car].bus.as_ref().unwrap().next_stop, 2, "the bus's next stop moved down one");
    assert_eq!(w.citizens[a].transit.map(|t| t.leg), Some(TransitLeg::Bus { route, board: 1, alight: 3 }), "indices past the gap shift");
    if b != a {
        assert!(w.citizens[b].transit.is_none() && matches!(w.citizens[b].activity, Activity::Idle), "a trip through the removed stop is cancelled");
    }
    // Removing a stop before the bus's target does not touch a bus heading home.
    w.traffic.cars[car].bus.as_mut().unwrap().next_stop = usize::MAX;
    w.remove_bus_stop(route, 0);
    assert_eq!(w.traffic.cars[car].bus.as_ref().unwrap().next_stop, usize::MAX);
    assert_eq!(w.citizens[a].transit.map(|t| t.leg), Some(TransitLeg::Bus { route, board: 0, alight: 2 }));
}

#[test]
fn brownouts_cut_production() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
    let ind: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial).map(|(id, _)| id).collect();
    let res: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, _)| id).collect();
    for &l in ind.iter().chain(res.iter()) {
        w.build_on(l);
    }
    finish_all(&mut w);
    // A fully staffed grain farm (no inputs, so output depends on power alone).
    let farm = w.lots[ind[0]].building.unwrap();
    {
        let b = &mut w.buildings[farm];
        b.tier = Tier::Low;
        b.jobs = 8;
        b.business = Some(Business::new(BusinessKind::GrainFarm, 8));
        b.workers = vec![CitizenId::default(); 8];
    }
    let full = w.buildings[farm].business.clone().unwrap().produce(8, 1.0);
    assert!(full > 0.0);
    // Lights on: an hour makes the full amount.
    let stock0 = w.buildings[farm].business.as_ref().unwrap().stock_of(Resource::Grain);
    w.hourly_economy();
    assert!(w.buildings[farm].powered >= 0.999, "powered {}", w.buildings[farm].powered);
    let made_lit = w.buildings[farm].business.as_ref().unwrap().stock_of(Resource::Grain) - stock0;
    assert!((made_lit - full).abs() < 1e-3, "lit: {made_lit} vs {full}");
    // Towers everywhere: demand outruns the grid and the cheap farm is
    // served last, so it makes less.
    for b in w.buildings.values_mut() {
        if b.lot != ind[0] {
            b.floors = 30;
            b.tier = Tier::High;
        }
    }
    let stock1 = w.buildings[farm].business.as_ref().unwrap().stock_of(Resource::Grain);
    w.hourly_economy();
    let p = w.buildings[farm].powered;
    assert!(p < 0.9, "the farm is browned out: {p}");
    assert!(w.economy.power.brownout_hours_today >= 1);
    let made_dark = w.buildings[farm].business.as_ref().unwrap().stock_of(Resource::Grain) - stock1;
    assert!(made_dark < made_lit * 0.9, "dark: {made_dark} vs lit {made_lit}");
    assert!((made_dark - full * p).abs() < 1e-3, "output scales with power: {made_dark} vs {}", full * p);
}

#[test]
fn job_seekers_staff_the_food_shops_first_and_arrivals_scale_with_open_homes() {
    use crate::market::{Business, BusinessKind};
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    let a = lot_near(&w, Vec2::new(-150.0, 20.0), LandUse::Commercial);
    let b = lot_near(&w, Vec2::new(-60.0, 20.0), LandUse::Commercial);
    w.build_on(a);
    w.build_on(b);
    let (grocer, gadgets) = (w.lots[a].building.unwrap(), w.lots[b].building.unwrap());
    for (id, kind) in [(grocer, BusinessKind::Grocery), (gadgets, BusinessKind::GadgetShop)] {
        w.finish_construction(id);
        let bld = &mut w.buildings[id];
        bld.jobs = 10;
        bld.workers.clear();
        bld.business = Some(Business::new(kind, 10));
    }
    // An empty grocer and an empty gadget shop: the grocer is picked every
    // time (food sellers get a half-staffing head start).
    for _ in 0..20 {
        assert_eq!(w.pick_job(), Some(grocer));
    }
    // A full grocer leaves only the gadget shop.
    w.buildings[grocer].workers = (0..10).map(|_| CitizenId::default()).collect();
    assert_eq!(w.pick_job(), Some(gadgets));
    // Arrivals grow with open homes and jobs, capped.
    assert!((1..=3).contains(&w.arrivals(5)));
    let many = w.arrivals(2_000);
    assert!(many >= 1 && many <= 12);
}

#[test]
fn imported_parts_come_from_out_of_town_not_from_the_site_itself() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Commercial);
    let lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Commercial);
    // Nobody in town sells parts: the developer imports them by truck.
    w.build_on(lot);
    let site = w.lots[lot].building.unwrap();
    let truck = w.traffic.cars.iter().find(|(_, c)| c.cargo.is_some_and(|k| k.to == site)).map(|(id, _)| id).expect("a parts truck on the road");
    let cargo = w.traffic.cars[truck].cargo.unwrap();
    assert_ne!(cargo.from, cargo.to, "an import does not claim to come from the buyer");
    assert!(w.buildings.get(cargo.from).is_none(), "it comes from out of town");
    assert_eq!(cargo.price, 0.0, "the developer paid for site parts up front");
    let mut minds = IdleMind;
    for _ in 0..120 {
        w.tick(&mut minds);
        if !w.traffic.cars.contains_key(truck) {
            break;
        }
    }
    assert!(!w.traffic.cars.contains_key(truck), "the truck drove in from the edge and arrived");
    assert!(w.buildings[site].construction.as_ref().is_some_and(|c| c.deliveries >= 1), "the parts reached the site");
}

#[test]
fn the_city_buys_out_grown_buildings_for_its_own_structures_but_not_each_other() {
    let mut w = World::new(WorldConfig::test_town());
    w.economy.treasury = 500_000.0;
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
    let lot = lot_near(&w, Vec2::new(100.0, 40.0), LandUse::Industrial);
    w.build_on(lot);
    let works = w.lots[lot].building.unwrap();
    w.finish_construction(works);
    let plant = w.build_power(lot, PowerPlantKind::Solar).expect("the plant takes over the works");
    assert!(!w.buildings.contains_key(works), "the works was bought out");
    assert_eq!(w.lots[lot].building, Some(plant));
    assert!(w.drain_events().iter().any(|e| matches!(e, SimEvent::Expropriated(b, _) if *b == works)));
    // A second plant on the same lot is refused, and the first one stays.
    let err = w.build_power(lot, PowerPlantKind::Coal).expect_err("a plant never replaces a plant");
    assert!(err.contains("bulldoze"), "{err}");
    assert!(w.buildings.contains_key(plant));
    let err = w.build_depot(lot).expect_err("a depot never replaces a plant");
    assert!(err.contains("bulldoze"), "{err}");
}

// ── households, opening hours, sleep, food and rent (feat/people) ──────────

/// A street of finished homes with a grocer at the end; ticks until a few
/// households have moved in.
fn family_street() -> (World, BuildingId) {
    use crate::market::{Business, BusinessKind};
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    for x in [40.0, 100.0, 160.0] {
        let lot = lot_near(&w, Vec2::new(x, 20.0), LandUse::Residential);
        w.build_on(lot);
    }
    let shop_lot = lot_near(&w, Vec2::new(-100.0, 20.0), LandUse::Commercial);
    w.build_on(shop_lot);
    finish_all(&mut w);
    let shop = w.lots[shop_lot].building.unwrap();
    let jobs = w.buildings[shop].jobs;
    w.buildings[shop].business = Some(Business::new(BusinessKind::Grocery, jobs));
    w.buildings[shop].business.as_mut().unwrap().add(Resource::Groceries, 200.0);
    let mut minds = IdleMind;
    for _ in 0..900 {
        if w.households.len() >= 4 {
            break;
        }
        w.tick(&mut minds);
    }
    assert!(w.households.len() >= 2, "households moved in");
    (w, shop)
}

#[test]
fn households_arrive_together_share_a_home_and_only_adults_work() {
    let (mut w, _) = family_street();
    for (hid, hh) in w.households.iter() {
        assert!((1..=5).contains(&hh.size()), "one to five members");
        let home = hh.home.expect("housed");
        assert!(w.buildings[home].households.contains(&hid), "the home lists its household");
        for &m in &hh.members {
            let c = &w.citizens[m];
            assert_eq!(c.household, Some(hid));
            assert_eq!(c.home, Some(home), "everyone lives at the household's home");
            assert!(w.buildings[home].residents.contains(&m));
            match c.stage() {
                crate::citizens::LifeStage::Child => assert!(c.work.is_none() && c.income == 0.0, "children neither work nor earn"),
                crate::citizens::LifeStage::Retiree => assert!(c.work.is_none() && c.income == crate::citizens::PENSION),
                crate::citizens::LifeStage::Adult => assert!(c.income > 0.0),
            }
        }
    }
    // A home holds one household, however many people that is.
    for b in w.buildings.values() {
        assert!(b.households.len() <= b.homes as usize);
        assert_eq!(b.free_homes() as usize, b.homes as usize - b.households.len());
    }
    // Arrivals come as singles, couples and families: children always with
    // a grown-up, every size from one to five.
    let mut sizes = [0u32; 6];
    for _ in 0..400 {
        let ages = w.household_ages();
        assert!((1..=5).contains(&ages.len()));
        assert!(ages.iter().any(|&a| a >= crate::citizens::ADULT_AGE), "nobody arrives without a grown-up: {ages:?}");
        sizes[ages.len()] += 1;
    }
    assert!(sizes[1..].iter().all(|&n| n > 0), "{sizes:?}");
    // The job search passes children by.
    let kid = w.citizens.iter().find(|(_, c)| c.stage() == crate::citizens::LifeStage::Child).map(|(id, _)| id);
    if let Some(kid) = kid {
        w.job_search();
        assert!(w.citizens[kid].work.is_none());
    }
    w.update_economy();
    let e = &w.economy;
    assert_eq!(e.households as usize, w.households.len());
    assert_eq!(e.household_sizes.iter().sum::<u32>() as usize, w.households.len(), "the size histogram counts every household");
}

#[test]
fn groceries_and_rent_come_out_of_the_household_purse() {
    let (mut w, shop) = family_street();
    let hid = w.households.keys().next().unwrap();
    let shopper = w.households[hid].members[0];
    w.households[hid].money = 500.0;
    w.households[hid].groceries = 0.0;
    w.citizens[shopper].savings = 7.0;
    let target = w.households[hid].pantry_target();
    w.buy(shopper, shop, Resource::Groceries);
    let hh = &w.households[hid];
    assert!(hh.groceries >= target.min(15.0).floor() - 0.01, "a shop for the whole household: {} of {target}", hh.groceries);
    assert!(hh.money < 500.0, "paid from the shared purse");
    assert_eq!(w.citizens[shopper].savings, 7.0, "not from the shopper's pocket");
    for &m in &hh.members {
        assert_eq!(w.citizens[m].groceries, hh.groceries, "every member sees the pantry");
    }
    // The day's books: incomes in, rent and living costs out, pockets pooled
    // and handed out again.
    let rent = w.buildings[w.households[hid].home.unwrap()].rent;
    let income = w.household_income(hid);
    let members = w.households[hid].members.clone();
    let living: f32 = members.iter().map(|m| if w.citizens[*m].stage() == crate::citizens::LifeStage::Child { 10.0 } else { 20.0 }).sum();
    let pockets: f32 = members.iter().map(|m| w.citizens[*m].savings).sum();
    // [carlife] Nobody buys wheels today: they have them.
    for &m in &members {
        w.citizens[m].owns_bike = true;
        if w.citizens[m].car.is_none() && w.citizens[m].stage() != crate::citizens::LifeStage::Child {
            let at = w.citizens[m].pos;
            w.give_car_parked(m, at);
        }
    }
    let before = w.households[hid].money + pockets;
    w.daily_household(hid);
    let pockets_after: f32 = members.iter().map(|m| w.citizens[*m].savings).sum();
    let after = w.households[hid].money + pockets_after;
    assert!((after - (before + income - rent - living)).abs() < 1.0, "money is conserved: {before} + {income} - {rent} - {living} != {after}");
    assert!(members.iter().all(|m| w.citizens[*m].savings <= crate::citizens::POCKET_MONEY + 0.01));
}

#[test]
fn old_saves_get_one_household_per_citizen() {
    let (mut w, _) = family_street();
    let people = w.citizens.len();
    // What a save from before households looks like once loaded: nobody
    // belongs to a household and no building lists any.
    w.households.clear();
    for c in w.citizens.values_mut() {
        c.household = None;
        c.savings = 55.0;
    }
    for b in w.buildings.values_mut() {
        b.households.clear();
    }
    let text = w.to_ron().unwrap();
    let back = World::from_ron(&text).expect("loads");
    assert_eq!(back.households.len(), people, "one household per citizen");
    for (cid, c) in back.citizens.iter() {
        let hid = c.household.expect("everyone belongs somewhere");
        let hh = &back.households[hid];
        assert_eq!(hh.members, vec![cid]);
        assert_eq!(hh.home, c.home);
        assert_eq!(hh.money, 55.0, "their savings became the purse");
        if let Some(home) = c.home {
            assert!(back.buildings[home].households.contains(&hid));
        }
    }
    // And a save with households round-trips them.
    let again = World::from_ron(&back.to_ron().unwrap()).unwrap();
    assert_eq!(again.households.len(), back.households.len());
}

#[test]
fn shops_keep_hours_and_turn_people_away_when_shut_or_full() {
    let (mut w, shop) = family_street();
    let at = w.building_entrance(shop).unwrap();
    let c = w.citizens.keys().next().unwrap();
    let day = w.clock.day();
    w.clock = SimClock::starting_at(day, 23, 0);
    assert_eq!(w.best_shop(Resource::Groceries, at), None, "grocers shut at 22:00");
    w.clock = SimClock::starting_at(day, 10, 0);
    assert_eq!(w.best_shop(Resource::Groceries, at), Some(shop));
    let cap = w.buildings[shop].customer_capacity();
    assert!(cap >= 8 && cap < u16::MAX);
    w.buildings[shop].visitors = cap;
    assert_eq!(w.best_shop(Resource::Groceries, at), None, "a full shop is skipped");
    // Arriving at a full shop: remembered, and off somewhere else.
    let go = |w: &mut World| {
        w.citizens[c].inside = None;
        w.citizens[c].activity = Activity::Traveling { to: Some(shop), purpose: Purpose::Shop(Resource::Groceries), path: vec![at], idx: 1 };
        w.arrive(c);
    };
    go(&mut w);
    assert!(matches!(w.citizens[c].activity, Activity::Idle));
    assert!(w.citizens[c].memories.last().unwrap().text.contains("packed"));
    // Arriving after closing time.
    w.buildings[shop].visitors = 0;
    w.clock = SimClock::starting_at(day, 22, 30);
    go(&mut w);
    assert!(matches!(w.citizens[c].activity, Activity::Idle));
    assert!(w.citizens[c].memories.last().unwrap().text.contains("was shut when I got there"), "{:?}", w.citizens[c].memories.last());
    assert!(w.economy.market.turned_away_today >= 2);
    // Open, with room: in they go.
    w.clock = SimClock::starting_at(day, 9, 0);
    go(&mut w);
    assert_eq!(w.citizens[c].inside, Some(shop));
    // Shifts follow the hours: the grocer's day is split in two.
    let b = &w.buildings[shop];
    let (early, late) = (b.shift_for(0), b.shift_for(1));
    assert_eq!((early.open, late.close), (7, 22));
    assert_eq!(early.close, late.open);
}

#[test]
fn sleep_restores_at_home_and_exhaustion_costs_output_and_mood() {
    let (mut w, shop) = family_street();
    let ids: Vec<CitizenId> = w.citizens.keys().take(2).collect();
    let (sleeper, worker) = (ids[0], ids.get(1).copied().unwrap_or(ids[0]));
    let day = w.clock.day();
    w.clock = SimClock::starting_at(day, 23, 0);
    {
        let c = &mut w.citizens[sleeper];
        c.inside = c.home;
        c.activity = Activity::AtHome;
        c.wakefulness = 0.1;
    }
    w.sleep_and_eat();
    assert!(w.citizens[sleeper].asleep, "at home at night: asleep");
    let w1 = w.citizens[sleeper].wakefulness;
    assert!(w1 > 0.1);
    for _ in 0..7 {
        w.sleep_and_eat();
    }
    assert!(w.citizens[sleeper].wakefulness > 0.9, "a night's sleep restores");
    w.clock = SimClock::starting_at(day + 1, 8, 0);
    w.sleep_and_eat();
    assert!(!w.citizens[sleeper].asleep, "rested and it is day: awake");
    // Working exhausted: half a shift's output and a worse mood.
    if worker != sleeper {
        {
            let c = &mut w.citizens[worker];
            c.work = Some(shop);
            c.inside = Some(shop);
            c.activity = Activity::Working;
            c.wakefulness = 0.05;
        }
        let mood = w.citizens[worker].mood;
        let tired = w.sleep_and_eat();
        assert_eq!(tired.get(&shop), Some(&1));
        assert!(w.citizens[worker].mood < mood);
        assert!(w.citizens[worker].productivity() < 1.0);
    }
    // Hungry away from home: the packed meal.
    let c = &mut w.citizens[sleeper];
    c.inside = None;
    c.activity = Activity::Idle;
    c.satiety = 0.3;
    c.packed_meal = 1.0;
    w.sleep_and_eat();
    assert!(w.citizens[sleeper].satiety > 0.6 && w.citizens[sleeper].packed_meal == 0.0);
}

#[test]
fn rent_stressed_households_move_somewhere_cheaper_before_leaving() {
    let (mut w, _) = family_street();
    let homes: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.land_use == LandUse::Residential).map(|(id, _)| id).collect();
    assert!(homes.len() >= 2);
    let hid = w.households.keys().next().unwrap();
    let home = w.households[hid].home.unwrap();
    let cheap = *homes.iter().find(|&&h| h != home).unwrap();
    for &m in &w.households[hid].members.clone() {
        let c = &mut w.citizens[m];
        c.income = if c.stage() == crate::citizens::LifeStage::Adult { 50.0 } else { 0.0 };
    }
    let income = w.household_income(hid);
    for &h in &homes {
        w.buildings[h].rent = income * 0.8;
    }
    w.buildings[cheap].rent = income * 0.2;
    w.buildings[cheap].homes = w.buildings[cheap].households.len() as u16 + 2;
    w.households[hid].money = -500.0;
    w.households[hid].moved_day = u64::MAX;
    let displaced = w.economy.displaced_total;
    w.daily_household(hid);
    assert!(w.households.contains_key(hid), "nobody left: there was somewhere cheaper");
    assert_eq!(w.households[hid].home, Some(cheap));
    assert!(w.buildings[cheap].households.contains(&hid) && !w.buildings[home].households.contains(&hid));
    assert!(w.economy.displaced_total > displaced);
    let m = w.households[hid].members[0];
    assert!(w.citizens[m].memories.last().unwrap().text.contains("Rent got too steep"));
    // Choosing a home: the best tier at most 35% of income where possible.
    w.buildings[cheap].rent = income * 0.3;
    w.buildings[home].rent = income * 0.45;
    let picks: Vec<Option<BuildingId>> = (0..10).map(|_| w.choose_home(income, None)).collect();
    assert!(picks.iter().all(|p| p.is_none_or(|b| w.buildings[b].rent <= income * 0.35)), "an affordable home first");
    // With nothing cheaper anywhere and the money gone, the household leaves.
    for &h in &homes {
        w.buildings[h].rent = income * 0.9;
    }
    w.households[hid].rent_stress_days = 5;
    w.households[hid].money = -5000.0;
    w.households[hid].moved_day = u64::MAX;
    w.daily_household(hid);
    // [society] ... or onto the street, when there is cheap ground to camp on.
    assert!(!w.households.contains_key(hid) || w.households[hid].home.is_none(), "out of town (or on the street) at last");
}

#[test]
fn low_income_demand_builds_low_tier_homes() {
    let mut w = World::new(WorldConfig::test_town());
    let picks = |w: &mut World| (0..200).filter(|_| w.residential_tier(Tier::High) == Tier::Low).count();
    assert_eq!(picks(&mut w), 0, "no demand: the land's tier");
    w.economy.housing_demand_by_tier = [20.0, 0.0, 0.0];
    let low = picks(&mut w);
    assert!(low > 120, "low-income demand pulls dear land down to cheap housing ({low}/200)");
    assert_eq!(w.residential_tier(Tier::Low), Tier::Low);
}


// ── planning: projects, pre-cut parcels, the zone brush ─────────────────

fn street(x0: f32, y0: f32, x1: f32, y1: f32) -> Gesture {
    Gesture::Road { points: vec![Vec2::new(x0, y0), Vec2::new(x1, y1)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() }
}

#[test]
fn several_projects_keep_their_own_history_and_implement_in_turn() {
    let mut w = flat_world();
    let main_road = w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    // Project A: a new street and a widened main road.
    let a = w.new_project(Some("Docks"));
    let a_street = w.add_gesture(street(100.0, -300.0, 100.0, 300.0));
    w.set_gesture(main_road, Gesture::Road { points: vec![Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], lanes_fwd: 2, lanes_bwd: 2, curved: false, kind: RoadKind::Street, elevation: Vec::new() }, true);
    // Project B: another street, and the main road changed differently.
    let b = w.new_project(None);
    assert_ne!(a, b);
    assert_eq!(w.project.as_ref().unwrap().id, b, "a new project becomes the active one");
    let b_street = w.add_gesture(street(-100.0, -300.0, -100.0, 300.0));
    w.set_gesture(main_road, Gesture::Road { points: vec![Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], lanes_fwd: 3, lanes_bwd: 3, curved: false, kind: RoadKind::Street, elevation: Vec::new() }, true);
    let list = w.projects();
    assert_eq!(list.len(), 2);
    assert_eq!(list.iter().find(|i| i.id == a).unwrap().name, "Docks");
    assert!(list.iter().all(|i| i.conflicts.iter().any(|(g, _)| *g == main_road)), "both touch the main road: {list:?}");
    // The active plan is B's; A is previewed dim.
    assert!(w.gesture(b_street).is_some() && w.gesture(a_street).is_none());
    let dim = w.inactive_previews();
    assert_eq!(dim.len(), 1);
    assert_eq!(dim[0].id, a);
    assert_eq!(dim[0].preview.planned_roads.len(), 2, "A's new street and its widened road");
    // Undo in B leaves A alone.
    assert!(w.undo() && w.undo());
    assert_eq!(w.project.as_ref().unwrap().steps.len(), 0);
    assert_eq!(w.projects().iter().find(|i| i.id == a).unwrap().steps, 2);
    assert!(w.redo() && w.redo());
    // Undo by id works on a parked project too, and redo brings it back.
    assert!(w.undo_project(a));
    assert_eq!(w.projects().iter().find(|i| i.id == a).unwrap().steps, 1);
    assert!(w.redo_project(a));
    // Switch back to A: its plan is the effective one now.
    assert!(w.switch_project(a));
    assert!(w.gesture(a_street).is_some() && w.gesture(b_street).is_none());
    assert!(w.rename_project(b, "Westside"));
    // Implement A first, then B: B's version of the shared road wins, and
    // the conflict is reported.
    let warn_a = w.implement_project(a).unwrap();
    assert!(warn_a.iter().any(|m| m.contains("Westside")), "{warn_a:?}");
    assert!(w.master.gestures.contains_key(&a_street));
    assert_eq!(w.projects().len(), 1, "A is gone, B still pending");
    assert!(!w.has_project(), "B was parked when A became active");
    let warn_b = w.implement_project(b).unwrap();
    assert!(warn_b.is_empty(), "nothing left to conflict with: {warn_b:?}");
    match &w.master.gestures[&main_road] {
        Gesture::Road { lanes_fwd, .. } => assert_eq!(*lanes_fwd, 3, "implemented later, B's road is built"),
        g => panic!("{g:?}"),
    }
    assert!(w.master.gestures.contains_key(&b_street));
    assert!(w.projects().is_empty());
    // Park and delete.
    let c = w.new_project(Some("Scrap"));
    w.add_gesture(street(0.0, 100.0, 0.0, 300.0));
    assert!(w.park_project() && !w.has_project());
    assert!(w.delete_project(c) && w.projects().is_empty());
}

#[test]
fn projects_survive_a_save_and_an_old_single_project_is_named() {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    let a = w.new_project(Some("North"));
    w.add_gesture(street(100.0, 0.0, 100.0, 300.0));
    let b = w.new_project(Some("South"));
    w.add_gesture(street(-100.0, 0.0, -100.0, -300.0));
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    let list = back.projects();
    assert_eq!(list.iter().map(|i| (i.id, i.name.as_str(), i.steps, i.active)).collect::<Vec<_>>(), vec![(a, "North", 1, false), (b, "South", 1, true)]);
    // A save from before named projects: one project, no id or name, and
    // none of the new fields.
    let mut old = flat_world();
    old.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    old.add_gesture(street(100.0, 0.0, 100.0, 300.0));
    old.project.as_mut().unwrap().id = 0;
    old.project.as_mut().unwrap().name.clear();
    let text = old.to_ron().unwrap();
    let text: String = text.lines().filter(|l| !l.trim_start().starts_with("parked_projects:") && !l.trim_start().starts_with("next_project:") && !l.trim_start().starts_with("parcel_lots:")).collect::<Vec<_>>().join("\n");
    let back = World::from_ron(&text).unwrap();
    let p = back.project.as_ref().expect("the pending project survives");
    assert!(p.id > 0 && p.name == "Project 1" && p.steps.len() == 1, "{} {:?}", p.id, p.name);
    assert!(back.parked_projects.is_empty());
}

#[test]
fn parcels_are_cut_along_a_fresh_road_with_no_zone() {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let along: Vec<&crate::zoning::Parcel> = w.parcels.parcels.iter().filter(|p| p.center.x.abs() < 280.0 && p.center.y.abs() < 40.0).collect();
    let north = along.iter().filter(|p| p.center.y > 0.0 && p.street_frontage).count();
    let south = along.iter().filter(|p| p.center.y < 0.0 && p.street_frontage).count();
    assert!(north >= 25 && south >= 25, "a row of parcels on each side: {north} / {south}");
    assert!(w.parcels.parcels.iter().all(|p| p.land_use.is_none() && p.lot.is_none()), "nothing is zoned");
    assert!(w.lots.values().all(|l| !w.zones.contains_key(l.zone)), "no zoned lots before zoning");
    assert!(w.parcels.parcels.iter().any(|p| p.landlocked()), "land behind the row is backland");
    assert!(w.planned_zoning().iter().all(|z| z.is_none()));
    // Keys are unique and find their parcel.
    for (i, p) in w.parcels.parcels.iter().enumerate() {
        assert_eq!(w.parcels.find(&p.key), Some(i));
    }
}

#[test]
fn a_zone_polygon_maps_onto_the_parcels_whose_centre_it_holds() {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_path(&[Vec2::new(-300.0, 80.0), Vec2::new(300.0, 80.0)]);
    let poly = square(-100.0, 5.0, 200.0);
    w.add_zone(poly.clone(), LandUse::Commercial);
    for (i, p) in w.parcels.parcels.iter().enumerate() {
        let inside = crate::zoning::point_in_polygon(p.center, &poly);
        let expect = inside && p.accepts(LandUse::Commercial);
        assert_eq!(p.land_use == Some(LandUse::Commercial), expect, "parcel {i} at {:?}", p.center);
        assert_eq!(w.planned_zoning()[i], p.land_use);
    }
    // Shops need a street: the parcels fronting only the path stay unzoned.
    assert!(w.parcels.parcels.iter().any(|p| crate::zoning::point_in_polygon(p.center, &poly) && p.path_frontage && !p.street_frontage && p.land_use.is_none()));
    let lots: Vec<&Lot> = w.lots.values().filter(|l| l.land_use == LandUse::Commercial).collect();
    assert!(lots.len() >= 10 && lots.iter().all(|l| l.usable() && l.car_access()), "{} commercial lots", lots.len());
    assert!(lots.iter().all(|l| w.zones.contains_key(l.zone)), "lots belong to the zone");
    // Homes do line the path.
    w.add_zone(poly.clone(), LandUse::Residential);
    assert!(w.lots.values().any(|l| l.land_use == LandUse::Residential && !l.car_access() && l.usable()), "homes front the footpath");
}

#[test]
fn the_brush_paints_only_parcels_that_take_the_use_and_one_stroke_is_one_undo() {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_path(&[Vec2::new(-300.0, 80.0), Vec2::new(300.0, 80.0)]);
    // One stroke from the street row up across the path row.
    let mut stroke = None;
    let mut seen: Vec<usize> = Vec::new();
    for k in 0..12 {
        let at = Vec2::new(-60.0 + k as f32 * 10.0, 10.0 + k as f32 * 7.0);
        let ids = w.parcels.in_circle(at, 12.0);
        seen.extend(&ids);
        stroke = w.paint_parcels(&ids, Some(LandUse::Commercial), stroke).0;
    }
    let stroke = stroke.expect("something was painted");
    assert_eq!(w.project.as_ref().unwrap().steps.len(), 1, "the whole stroke is one step");
    seen.sort();
    seen.dedup();
    let planned = w.planned_zoning().to_vec();
    let mut refused = 0;
    for &i in &seen {
        let p = &w.parcels.parcels[i];
        if p.accepts(LandUse::Commercial) {
            assert_eq!(planned[i], Some(LandUse::Commercial));
        } else {
            refused += 1;
            assert_eq!(planned[i], None, "parcel {i} refuses shops: {:?}", p.refusal(LandUse::Commercial));
        }
    }
    assert!(refused > 0, "path-fronting parcels and backland refuse shops");
    // Parks go anywhere.
    assert!(seen.iter().all(|&i| w.parcels.parcels[i].accepts(LandUse::Park)));
    // Undo takes the stroke back whole, redo restores it.
    assert!(w.undo());
    assert!(w.planned_zoning().iter().all(|z| z.is_none()));
    assert!(w.gesture(stroke).is_none());
    assert!(w.redo());
    w.implement();
    assert!(w.lots.values().filter(|l| l.land_use == LandUse::Commercial).count() >= 2);
    // Industry joins parcels into bigger lots and takes the backland behind.
    let ids = w.parcels.in_polygon(&square(100.0, -100.0, 160.0));
    w.paint_parcels(&ids, Some(LandUse::Industrial), None);
    assert!(ids.iter().any(|&i| w.parcels.parcels[i].landlocked() && w.planned_zoning()[i] == Some(LandUse::Industrial)), "works yards take the backland");
    w.implement();
    let ind: Vec<&Lot> = w.lots.values().filter(|l| l.land_use == LandUse::Industrial && l.usable()).collect();
    assert!(!ind.is_empty());
    let mean = ind.iter().map(|l| l.area()).sum::<f32>() / ind.len() as f32;
    let parcel_mean = ids.iter().map(|&i| w.parcels.parcels[i].area).sum::<f32>() / ids.len() as f32;
    assert!(mean > parcel_mean * 1.5, "industrial lots are joined parcels: mean {mean:.0} m² vs parcels {parcel_mean:.0}");
    // Erasing unzones.
    let (_, n) = w.paint_parcels(&ids, None, None);
    assert!(n > 0);
    w.implement();
    assert!(w.lots.values().all(|l| l.land_use != LandUse::Industrial));
}

#[test]
fn painted_parcels_keep_their_zone_through_rebuilds() {
    let mut w = flat_world();
    let road = w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    let ids = w.parcels.in_circle(Vec2::new(150.0, 15.0), 30.0);
    let (_, n) = w.paint_parcels(&ids, Some(LandUse::Residential), None);
    assert!(n >= 3);
    w.implement();
    let homes: Vec<(crate::zoning::ParcelKey, Vec2)> = w.parcels.parcels.iter().filter(|p| p.land_use == Some(LandUse::Residential)).map(|p| (p.key, p.center)).collect();
    assert!(homes.len() >= 3);
    let lot = w.lots.iter().find(|(_, l)| l.land_use == LandUse::Residential && l.usable()).map(|(id, _)| id).unwrap();
    w.build_on(lot);
    let b = w.lots[lot].building.unwrap();
    // Road works far away: lot ids change, the parcels and keys do not.
    w.add_road(&[Vec2::new(-300.0, -300.0), Vec2::new(-300.0, 300.0)], 2, 2);
    for (k, c) in &homes {
        let i = w.parcels.find(k).expect("the key still names a parcel");
        assert_eq!(w.parcels.parcels[i].key, *k, "same parcel, same key");
        assert!(w.parcels.parcels[i].center.distance(*c) < 0.01);
        assert_eq!(w.parcels.parcels[i].land_use, Some(LandUse::Residential));
    }
    assert!(w.buildings.contains_key(b), "an untouched parcel keeps its building");
    // The road widened under them reshapes the parcels: the keys are found
    // again by their old centres and the zone carries over, while the
    // building whose parcel changed shape is pulled down.
    w.set_gesture(road, Gesture::Road { points: vec![Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], lanes_fwd: 3, lanes_bwd: 3, curved: false, kind: RoadKind::Street, elevation: Vec::new() }, true);
    w.implement();
    assert!(!w.buildings.contains_key(b), "eminent domain still applies");
    let still = homes.iter().filter(|(k, _)| w.parcels.find(k).is_some_and(|i| w.parcels.parcels[i].land_use == Some(LandUse::Residential))).count();
    assert!(still * 10 >= homes.len() * 7, "most painted parcels are still homes after the widening: {still} of {}", homes.len());
}

#[test]
fn an_industrial_sub_zone_rides_on_the_stroke_and_one_undo_takes_both() {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let poly = square(-100.0, 5.0, 100.0);
    let zone = w.add_zone_gesture(poly.clone(), LandUse::Industrial, Some("logistics"));
    let step = &w.project.as_ref().unwrap().steps[0];
    assert_eq!(w.project.as_ref().unwrap().steps.len(), 1, "zone and sub-zone are one step");
    assert!(step.contains_key(&zone) && step.values().flatten().any(|g| matches!(g, Gesture::ParcelZone { industry: Some(k), .. } if k == "logistics")));
    assert!(w.undo() && w.effective_plan().gestures.len() == w.master.gestures.len());
    assert!(w.redo());
    // Repainting industrial land with a sub-zone repaints it even though
    // the land use is unchanged.
    w.implement();
    let ids = w.parcels.in_polygon(&poly);
    let (_, n) = w.paint_parcels_with(&ids, Some(LandUse::Industrial), Some("tech"), None);
    assert!(n > 0);
    let (_, n) = w.paint_parcels(&ids, Some(LandUse::Industrial), None);
    assert_eq!(n, 0, "without a sub-zone, already industrial parcels are left alone");
}

/// A street of homes facing a grocer, every household short of food and
/// flush, the clock at mid-morning; land value set on every building.
fn delivery_street(land: f32) -> (World, BuildingId) {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_zone(square(-150.0, 8.0, 300.0), LandUse::Residential);
    w.add_zone(square(-150.0, -308.0, 300.0), LandUse::Commercial);
    let shop_lot = lot_near(&w, Vec2::new(0.0, -20.0), LandUse::Commercial);
    w.build_on(shop_lot);
    for x in [-90.0f32, -40.0, 40.0, 90.0] {
        let l = lot_near(&w, Vec2::new(x, 20.0), LandUse::Residential);
        w.build_on(l);
    }
    finish_all(&mut w);
    let shop = w.lots[shop_lot].building.unwrap();
    {
        let b = &mut w.buildings[shop];
        let mut bz = crate::market::Business::new(crate::market::BusinessKind::Grocery, b.jobs.max(4));
        bz.add(Resource::Groceries, 200.0);
        b.business = Some(bz);
    }
    let homes: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.land_use == LandUse::Residential).map(|(id, _)| id).collect();
    for h in homes {
        let hid = w.households.insert(Household { members: Vec::new(), home: Some(h), money: 5000.0, groceries: 0.0, rent_stress_days: 0, moved_day: 0 });
        w.buildings[h].households.push(hid);
    }
    for b in w.buildings.values_mut() {
        b.land_value = land;
    }
    w.clock = crate::clock::SimClock::starting_at(0, 10, 0);
    (w, shop)
}

/// [camera] Every robot knows its shop, its number in the shop's fleet, the
/// household that ordered, what it carries and where it is going; the
/// fleet's colour is the shop's own; a save keeps it all.
#[test]
fn delivery_bots_carry_their_owner_number_and_cargo() {
    let (mut w, shop) = delivery_street(0.9);
    for _ in 0..20 {
        w.clock.tick += 15;
        w.dispatch_deliveries();
    }
    let fleet = w.bots.fleet_of(shop);
    assert!(!fleet.is_empty(), "the shop sent robots out");
    let serials: Vec<u32> = fleet.iter().map(|&id| w.bots.bots[id].serial).collect();
    assert_eq!(serials, (1..=fleet.len() as u32).collect::<Vec<_>>(), "numbered 1, 2, 3 in the fleet");
    for &id in &fleet {
        let b = &w.bots.bots[id];
        assert_eq!(b.shop, shop);
        assert_eq!(b.destination(), b.home, "heading to the customer");
        let (res, n) = b.cargo().expect("carrying the order");
        assert_eq!(res, Resource::Groceries);
        assert!(n > 0.0);
        let hh = b.household.expect("an order has a household");
        assert_eq!(w.households[hh].home, Some(b.home), "the household lives at the door it goes to");
        assert!(b.metres_left() > 0.0);
        assert!(crate::bots::bot_name(&w, id).starts_with(&w.buildings[shop].name));
    }
    // The next free number fills a gap.
    let first = fleet[0];
    w.bots.bots.remove(first);
    assert_eq!(w.bots.next_serial(shop), 1);
    // Each company its own colour, the same every time.
    let other = w.buildings.keys().find(|&b| b != shop).unwrap();
    assert_eq!(crate::bots::fleet_colour(shop), crate::bots::fleet_colour(shop));
    assert_ne!(crate::bots::fleet_colour(shop), crate::bots::fleet_colour(other));
    // Delivered: empty-handed, heading home to the shop.
    let id = fleet[1.min(fleet.len() - 1)];
    if let Some(b) = w.bots.bots.get_mut(id) {
        b.idx = b.path.len();
    }
    w.step_bots();
    if let Some(b) = w.bots.bots.get(id) {
        assert!(b.returning);
        assert_eq!(b.destination(), shop);
        assert!(b.cargo().is_none(), "the order was handed over");
    }
    // A save keeps the numbers and orders.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    for (id, b) in w.bots.bots.iter() {
        assert_eq!(back.bots.bots[id].serial, b.serial);
        assert_eq!(back.bots.bots[id].household, b.household);
    }
}

#[test]
fn delivery_bots_work_only_on_dear_streets() {
    let (mut rich, shop) = delivery_street(0.9);
    let stock0 = rich.buildings[shop].business.as_ref().unwrap().stock_of(Resource::Groceries);
    for _ in 0..20 {
        rich.clock.tick += 15;
        rich.dispatch_deliveries();
    }
    assert!(!rich.bots.bots.is_empty(), "robots carry orders on a dear street");
    assert!(rich.buildings[shop].business.as_ref().unwrap().stock_of(Resource::Groceries) < stock0, "orders come off the shelves");
    assert!(rich.households.values().any(|h| h.groceries > 0.0), "and into a pantry");
    // They walk to the door and back, then park.
    let mut minds = IdleMind;
    let delivered0 = rich.bots.delivered_today;
    for _ in 0..300 {
        rich.tick(&mut minds);
    }
    assert!(rich.bots.delivered_today > delivered0 || rich.clock.minute_of_day() == 0, "bots reach the door");
    let max_lv = rich.bots.bots.values().map(|b| rich.buildings[b.shop].land_value.min(rich.buildings[b.home].land_value)).fold(1.0, f32::min);
    assert!(rich.bots.bots.is_empty() || max_lv >= crate::bots::BOT_LAND_VALUE);
    // A save keeps them.
    let back = World::from_ron(&rich.to_ron().unwrap()).unwrap();
    assert_eq!(back.bots.bots.len(), rich.bots.bots.len());

    let (mut poor, _) = delivery_street(0.2);
    for _ in 0..20 {
        poor.clock.tick += 15;
        poor.dispatch_deliveries();
    }
    assert!(poor.bots.bots.is_empty(), "no robots on cheap streets");
    assert!(poor.bots.van_orders_today > 0, "a van brings the shopping instead");
    assert!(poor.traffic.cars.values().any(|c| c.model.livery == crate::vehicle::Livery::Grocery), "in the grocer's livery");
}

#[test]
fn owners_drive_what_their_income_and_age_buy_and_saves_keep_it() {
    use crate::vehicle::{BodyType, VehicleClass, VehicleModel};
    let mut w = World::new(WorldConfig::test_town());
    for i in -1..=1 {
        let c = i as f32 * 200.0;
        w.add_road(&[Vec2::new(-500.0, c), Vec2::new(500.0, c)], 1, 1);
        w.add_road(&[Vec2::new(c, -500.0), Vec2::new(c, 500.0)], 1, 1);
    }
    w.add_zone(square(-190.0, -190.0, 180.0), LandUse::Residential);
    w.add_zone(square(10.0, 10.0, 180.0), LandUse::Commercial);
    let mut minds = IdleMind;
    for _ in 0..(24 * 60) {
        w.tick(&mut minds);
    }
    finish_all(&mut w);
    for _ in 0..(12 * 60) {
        w.tick(&mut minds);
    }
    assert!(!w.citizens.is_empty());
    // Every owner has a model, chosen by class.
    for c in w.citizens.values().filter(|c| c.owns_car) {
        let m = c.vehicle.expect("an owner has a vehicle");
        match c.class() {
            Tier::Low => assert_eq!(m.class, VehicleClass::Cheap),
            Tier::High => assert_ne!(m.class, VehicleClass::Cheap),
            _ => {}
        }
    }
    // A driven car looks like the owner's vehicle.
    let mut checked = false;
    for _ in 0..(2 * 24 * 60) {
        w.tick(&mut minds);
        if let Some(car) = w.traffic.cars.values().find(|c| w.citizens.contains_key(c.citizen)) {
            assert_eq!(Some(car.model), w.citizens[car.citizen].vehicle);
            checked = true;
            break;
        }
    }
    assert!(checked, "someone drove");
    // Old saves: an owner without a model gets one from the seed, the same
    // each time.
    let c = w.citizens.keys().next().unwrap();
    let (seed, income, age) = (w.citizens[c].seed, w.citizens[c].income, w.citizens[c].age);
    assert_eq!(VehicleModel::fallback(seed, income, age), VehicleModel::fallback(seed, income, age));
    // Save and load keep models on citizens and cars.
    let tuned = VehicleModel { body: BodyType::AkiraBike, class: VehicleClass::Expensive, ..Default::default() };
    w.citizens[c].vehicle = Some(tuned);
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.citizens[c].vehicle, Some(tuned));
    for (id, car) in w.traffic.cars.iter() {
        assert_eq!(back.traffic.cars[id].model, car.model);
    }
}

#[test]
fn rezoning_never_touches_city_structures_and_frees_parks_for_building() {
    let mut w = flat_world();
    w.config.emergencies = false;
    w.economy.treasury = 500_000.0;
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Industrial);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Park);
    // A plant on the industrial land.
    let plant_lot = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Industrial && l.usable()).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, _)| id).unwrap();
    let plant = w.build_power(plant_lot, PowerPlantKind::Solar).expect("plant");
    let plant_at = w.lots[plant_lot].center;
    // Painting homes over it is refused, with a reason, and a polygon zone
    // drawn over it changes nothing there.
    let under: Vec<usize> = w.parcels.in_circle(plant_at, 3.0);
    assert!(!under.is_empty());
    assert!(under.iter().all(|&i| w.parcel_refusal(i, Some(LandUse::Residential)).is_some_and(|why| why.contains("bulldoze"))));
    let (_, painted) = w.paint_parcels(&under, Some(LandUse::Residential), None);
    assert_eq!(painted, 0, "nothing under the plant takes the paint");
    w.start_project();
    w.add_zone_gesture(square(-200.0, 10.0, 190.0), LandUse::Residential, None);
    w.implement();
    assert!(w.buildings.contains_key(plant), "the plant survives a rezoning of its block");
    assert!(w.buildings[plant].plant.is_some());
    // A park, rezoned for homes: it stays until building begins, then goes.
    let park_lot = w.lots.iter().find(|(_, l)| l.land_use == LandUse::Park && l.usable()).map(|(id, _)| id).unwrap();
    w.build_on(park_lot);
    finish_all(&mut w);
    let park = w.lots[park_lot].building.expect("a park was laid out");
    let park_at = w.lots[park_lot].center;
    let ids = w.parcels.in_circle(park_at, 3.0);
    w.paint_parcels(&ids, Some(LandUse::Residential), None);
    w.implement();
    let lot = w.lots.iter().find(|(_, l)| l.contains(park_at)).map(|(id, _)| id).unwrap();
    assert_eq!(w.lots[lot].land_use, LandUse::Residential);
    assert_eq!(w.lots[lot].building, Some(park), "the park stands until building begins");
    w.build_on(lot);
    assert!(!w.buildings.contains_key(park), "building began: the park is cleared");
    let site = w.lots[lot].building.expect("a building site");
    assert_eq!(w.buildings[site].land_use, LandUse::Residential);
    // Unzoning a park leaves it standing.
    let other = w.lots.iter().find(|(_, l)| l.land_use == LandUse::Park && l.usable() && l.building.is_none()).map(|(id, _)| id).unwrap();
    w.build_on(other);
    finish_all(&mut w);
    let park2 = w.lots[other].building.unwrap();
    let at2 = w.lots[other].center;
    let ids = w.parcels.in_circle(at2, 3.0);
    w.paint_parcels(&ids, None, None);
    w.implement();
    assert!(w.buildings.contains_key(park2), "an unzoned park stays");
}


// ── looks (feat/looks) ─────────────────────────────────────────────────────

#[test]
fn looks_are_given_on_arrival_and_survive_save_and_load() {
    let (w, _) = family_street();
    assert!(w.citizens.values().all(|c| c.look.is_some()), "everyone arrives with a look");
    // Families arrive resembling each other: children share a parent's hair.
    for hh in w.households.values() {
        let kids: Vec<&Citizen> = hh.members.iter().map(|m| &w.citizens[*m]).filter(|c| c.age < crate::citizens::ADULT_AGE).collect();
        let adults: Vec<&Citizen> = hh.members.iter().map(|m| &w.citizens[*m]).filter(|c| c.age >= crate::citizens::ADULT_AGE).collect();
        if adults.is_empty() {
            continue;
        }
        for k in kids {
            let hair = k.look.unwrap().genes.hair;
            let parent_hair: Vec<_> = adults.iter().map(|a| a.look.unwrap().genes.hair).collect();
            let absent = adults.len() == 1;
            assert!(absent || parent_hair.contains(&hair), "{} has a parent's hair", k.name);
        }
    }
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    for (id, c) in w.citizens.iter() {
        assert_eq!(back.citizens[id].look, c.look, "{}'s look survives a load", c.name);
    }
}

#[test]
fn a_save_from_before_looks_gets_the_looks_it_would_have_had() {
    let (mut w, _) = family_street();
    let given: Vec<(CitizenId, crate::looks::Appearance)> = w.citizens.iter().map(|(id, c)| (id, c.look.unwrap())).collect();
    for c in w.citizens.values_mut() {
        c.look = None;
    }
    let text = w.to_ron().unwrap();
    assert!(!text.contains("genes"), "an old save has no looks");
    let back = World::from_ron(&text).unwrap();
    for (id, look) in given {
        let got = back.citizens[id].look.expect("filled on load");
        // The cut of the clothes follows the class at the time; the rest
        // is drawn from the seeds alone.
        assert_eq!(got.genes, look.genes);
        assert_eq!((got.style, got.dye, got.fav, got.build), (look.style, look.dye, look.fav, look.build));
    }
}

#[test]
fn touching_solar_and_wind_plants_form_one_field() {
    use crate::geom::shared_boundary;
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
    w.economy.treasury = 10_000_000.0;
    let big: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.area() >= PowerPlantKind::Wind.min_area()).map(|(id, _)| id).collect();
    // A chain of three parcels, each touching the next along a side.
    let touch = |w: &World, a: LotId, b: LotId| shared_boundary(&w.lots[a].polygon, &w.lots[b].polygon) >= 4.0;
    let chain = big.iter().find_map(|&a| {
        big.iter().filter(|&&b| b != a && touch(&w, a, b)).find_map(|&b| big.iter().find(|&&c| c != a && c != b && touch(&w, b, c) && !touch(&w, a, c)).map(|&c| (a, b, c)))
    });
    let (a, b, c) = chain.expect("three parcels in a row");
    let pa = w.build_power(a, PowerPlantKind::Solar).unwrap();
    let pb = w.build_power(b, PowerPlantKind::Solar).unwrap();
    for id in [pa, pb] {
        w.buildings[id].demolition = None;
    }
    let mut both = vec![a, b];
    both.sort();
    assert_eq!(w.plant_cluster(pa), both);
    assert_eq!(w.plant_cluster(pb), both, "every member sees the same field, anchor first");
    // A wind farm next door is a different field.
    let pc = w.build_power(c, PowerPlantKind::Wind).unwrap();
    w.buildings[pc].demolition = None;
    assert_eq!(w.plant_cluster(pc), vec![c]);
    assert_eq!(w.plant_cluster(pa), both);
    // Turned into solar it joins through b, though it never touches a.
    w.buildings[pc].plant = Some(PowerPlantKind::Solar);
    assert_eq!(w.plant_cluster(pa).len(), 3);
    // A site still being cleared is not part of the field yet.
    w.buildings[pc].demolition = Some(crate::buildings::Demolition { progress: 0.5, days: 1.0, loads: 0, floors: 1, tier: Tier::Low, land_use: LandUse::Industrial, seed: 1, condition: 1.0, clear_lot: false, own_lot: false, look: None });
    assert_eq!(w.plant_cluster(pa), both);
}

/// [access] The player's rule: a long walk to any park makes homes
/// cheaper, measured along the walk network, not as the crow flies.
#[test]
fn land_value_counts_the_walk_to_a_park_not_the_crow_flight() {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, ..WorldConfig::test_town() });
    // Two streets that never meet, 200 m apart.
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-400.0, 200.0), Vec2::new(400.0, 200.0)], 1, 1);
    w.add_zone(square(-390.0, 10.0, 60.0), LandUse::Park);
    w.add_zone(vec![Vec2::new(-320.0, 10.0), Vec2::new(390.0, 10.0), Vec2::new(390.0, 60.0), Vec2::new(-320.0, 60.0)], LandUse::Residential);
    w.add_zone(vec![Vec2::new(-390.0, 140.0), Vec2::new(-250.0, 140.0), Vec2::new(-250.0, 190.0), Vec2::new(-390.0, 140.0 + 50.0)], LandUse::Residential);
    let park_lot = w.lots.iter().find(|(_, l)| l.usable() && l.land_use == LandUse::Park).map(|(id, _)| id).expect("a park lot");
    w.build_on(park_lot);
    let park = w.lots[park_lot].building.unwrap();
    w.finish_construction(park);
    w.update_land_value();
    let homes: Vec<(LotId, Vec2)> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, l)| (id, l.center)).collect();
    let near = homes.iter().filter(|h| h.1.y < 100.0).min_by(|a, b| a.1.x.total_cmp(&b.1.x)).unwrap();
    let far = homes.iter().filter(|h| h.1.y < 100.0).max_by(|a, b| a.1.x.total_cmp(&b.1.x)).unwrap();
    let across = homes.iter().filter(|h| h.1.y > 100.0).min_by(|a, b| a.1.distance(w.lots[park_lot].center).total_cmp(&b.1.distance(w.lots[park_lot].center))).expect("a home on the other street");
    let access = |p: Vec2| w.land_value.access_at(p).expect("on the walk network");
    let (an, af, ax) = (access(near.1), access(far.1), access(across.1));
    let pn = an.park_min.expect("the park is a walk away");
    let pf = af.park_min.expect("the far end can walk there too");
    assert!(pn < 3.0, "next to the park: {pn} min");
    assert!(pf > pn + 5.0, "the far end of the street walks further: {pf} vs {pn} min");
    // About the street's length on foot.
    let metres = (far.1.x - w.lots[park_lot].center.x).abs();
    assert!(pf >= metres / (WALK_MPS * 60.0) * 0.8, "{pf} min for {metres} m");
    assert!(w.lots[park_lot].center.distance(across.1) < 250.0, "close as the crow flies");
    assert_eq!(ax.park_min, None, "but no way to walk there from the other street");
    assert!(an.nightlife_min.is_none(), "no nightlife anywhere yet");
    // Homes feel it; the grid includes it.
    assert!(an.value > af.value && af.value > ax.value, "{} {} {}", an.value, af.value, ax.value);
    assert!(w.lot_value(near.0) > w.lot_value(across.0));
    // Old saves have no access data and load fine; the next update fills it.
    let mut old = w.land_value.clone();
    old.park_min.clear();
    old.nightlife_min.clear();
    old.access.clear();
    assert!(old.access_at(near.1).is_none());
}

/// [clarity] Solar and wind fill their lot: a field on twice the land
/// costs, runs to and yields twice as much, so a megawatt costs the same.
#[test]
fn solar_and_wind_scale_with_their_lot_at_a_steady_cost_per_megawatt() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
    w.add_zone(square(-300.0, 10.0, 90.0), LandUse::Industrial);
    w.economy.treasury = 10_000_000.0;
    let mut lots: Vec<(LotId, f32)> = w.lots.iter().filter(|(_, l)| l.usable() && l.area() >= PowerPlantKind::Wind.min_area()).map(|(id, l)| (id, l.area())).collect();
    lots.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (small, big) = (lots[0], *lots.last().unwrap());
    assert!(big.1 > small.1 * 1.3, "lots of different sizes: {} and {}", small.1, big.1);
    for kind in [PowerPlantKind::Solar, PowerPlantKind::Wind] {
        let mut w = w.clone();
        let before = w.economy.treasury;
        let a = w.build_power(small.0, kind).unwrap();
        let paid_a = before - w.economy.treasury;
        let before = w.economy.treasury;
        let b = w.build_power(big.0, kind).unwrap();
        let paid_b = before - w.economy.treasury;
        let (ba, bb) = (&w.buildings[a], &w.buildings[b]);
        let (mw_a, mw_b) = (w.plant_capacity_mw(ba), w.plant_capacity_mw(bb));
        let ratio = big.1 / small.1;
        assert!((mw_b / mw_a - ratio).abs() < 0.01, "{kind:?}: output in proportion to the land");
        assert!((paid_a / mw_a - paid_b / mw_b).abs() < 1.0, "{kind:?}: the same build cost per MW ({} and {})", paid_a / mw_a, paid_b / mw_b);
        assert!((w.plant_upkeep(ba) / mw_a - w.plant_upkeep(bb) / mw_b).abs() < 0.01, "{kind:?}: the same upkeep per MW");
    }
    // [freight] Hydro and tidal are capped like the thermal plants.
    assert_eq!(PowerPlantKind::Hydro.size_scale(10_000.0), PowerPlantKind::Hydro.size_range().1);
}
