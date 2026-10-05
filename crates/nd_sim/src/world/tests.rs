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

#[test]
fn city_grows_and_round_trips_through_ron() {
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    let lot = w.lots.iter().find(|(_, l)| l.usable()).map(|(id, _)| id).unwrap();
    w.build_on(lot);
    let building = w.lots[lot].building.unwrap();

    let id = w.add_gesture(Gesture::Road { points: vec![Vec2::new(300.0, -300.0), Vec2::new(300.0, 300.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street });
    assert_eq!(w.preview().planned_roads.len(), 1);
    assert!(w.undo());
    assert_eq!(w.preview().planned_roads.len(), 0);
    assert!(w.redo());
    w.set_gesture(id, Gesture::Road { points: vec![Vec2::new(320.0, -300.0), Vec2::new(320.0, 300.0)], lanes_fwd: 2, lanes_bwd: 2, curved: false, kind: RoadKind::Street }, true);
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
    let mut w = World::new(WorldConfig::default());
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
    // The newer commercial zone still sits on top of the repainted one.
    assert!(w.lots.values().filter(|l| point_in_polygon(l.center, &tri)).all(|l| l.land_use == LandUse::Commercial));
}

#[test]
fn supply_chain_feeds_the_city() {
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
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
fn parking_lots_fill_charge_and_release() {
    let mut w = World::new(WorldConfig::default());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Commercial);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Commercial).map(|(id, _)| id).collect();
    assert!(lots.len() >= 2);
    // A shop next to a parking lot.
    w.build_on(lots[0]);
    let shop = w.lots[lots[0]].building.unwrap();
    w.finish_construction(shop);
    let lot = w.build_parking(lots[1]).expect("parking lot opens");
    assert!(w.buildings[lot].construction.is_none(), "parking is laid out at once");
    let cap = w.buildings[lot].parking_capacity;
    assert!(cap >= 4 && w.buildings[lot].is_parking() && w.buildings[lot].jobs == 2);
    assert!(w.buildings[lot].parking_price > 0.0);
    // Low-tier shops have no spaces of their own, so a driver pays at the lot.
    let mut minds = IdleMind;
    for _ in 0..(12 * 60) {
        w.tick(&mut minds);
    }
    finish_all(&mut w);
    for _ in 0..60 {
        w.tick(&mut minds);
    }
    let c = w.citizens.keys().next().expect("someone arrived");
    w.buildings[shop].parking_capacity = 0;
    {
        let cz = &mut w.citizens[c];
        cz.savings = 100.0;
        cz.activity = Activity::Driving { car: w.traffic.cars.keys().next().unwrap_or_default(), to: Some(shop), purpose: Purpose::Shop(Resource::Goods) };
    }
    let before_money = w.buildings[lot].business.as_ref().unwrap().money;
    let used0 = w.buildings[lot].parking_used;
    w.park_car(c);
    assert_eq!(w.buildings[lot].parking_used, used0 + 1);
    assert_eq!(w.citizens[c].parked_at, Some(lot));
    assert!(w.citizens[c].savings < 100.0, "the driver paid");
    assert!(w.buildings[lot].business.as_ref().unwrap().money > before_money, "the lot earned");
    // Leaving frees the space.
    w.citizens[c].inside = Some(shop);
    w.leave_building(c);
    assert_eq!(w.buildings[lot].parking_used, used0);
    // With the lot full, drivers circle and the shortage is counted.
    w.buildings[lot].parking_used = cap;
    let street0 = w.economy.parking_street_today;
    w.citizens[c].activity = Activity::Driving { car: w.traffic.cars.keys().next().unwrap_or_default(), to: Some(shop), purpose: Purpose::Shop(Resource::Goods) };
    w.park_car(c);
    assert_eq!(w.economy.parking_street_today, street0 + 1);
    assert!(w.citizens[c].parked_at.is_none());
}

#[test]
fn park_visits_end_on_a_bench_and_people_leave_on_foot() {
    let mut w = World::new(WorldConfig::default());
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
    assert!(w.lots[park_lot].park.as_ref().unwrap().benches.iter().any(|(b, _)| b.distance(at) < 0.5), "sitting on a planned bench");
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
    let mut w = World::new(WorldConfig::default());
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
    // Editing: moving and removing stops keeps the line sane; removing the
    // last stop removes the line and its buses.
    assert!(w.move_bus_stop(route, 1, Vec2::new(250.0, 4.0)));
    assert!(w.transit.routes[route].runnable());
    w.remove_bus_stop(route, 3);
    assert_eq!(w.transit.routes[route].stops.len(), 5);
    w.set_route_buses(route, 2);
    assert_eq!(w.transit.routes[route].buses, 2);
    w.remove_bus_route(route);
    assert!(w.transit.routes.is_empty());
    assert_eq!(w.traffic.cars.values().filter(|c| c.bus.is_some()).count(), 0);
    // Round trip keeps the depot.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert!(back.buildings[depot].depot);
}

#[test]
fn trains_call_at_stations_and_yards_bring_freight() {
    use crate::rail::{Facility, RailFacility, TrainKind};
    let mut w = World::new(WorldConfig::default());
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
    w.travel_to(c, Some(shop), Purpose::Shop(Resource::Goods));
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
    let queued0 = w.buildings[yard].rail_inbound.len();
    w.import(buyer, Resource::Parts, 10.0, 100.0);
    assert_eq!(w.buildings[yard].rail_inbound.len(), queued0 + 1, "the order waits at the yard");
    assert!(w.buildings[lone].rail_inbound.is_empty(), "the unconnected yard gets nothing");
    assert!((w.buildings[buyer].business.as_ref().unwrap().money - money0 - 20.0).abs() < 0.01, "rail import refunds a fifth");
    let mut freight_seen = false;
    for _ in 0..900 {
        w.tick(&mut minds);
        if w.rail.trains.values().any(|t| t.kind == TrainKind::Freight) {
            freight_seen = true;
        }
        if w.buildings[yard].rail_inbound.is_empty() && freight_seen {
            break;
        }
    }
    assert!(freight_seen && w.buildings[yard].rail_inbound.is_empty(), "freight train {freight_seen}, waiting {}", w.buildings[yard].rail_inbound.len());
    assert!(w.rail.stats.freight_in_today >= 10.0);
    // Round trip keeps the facilities and their lots.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert!(back.buildings[station_a].rail.is_some() && back.buildings[yard].rail.is_some());
    assert_eq!(back.facilities.len(), 4);
}

#[test]
fn power_is_rationed_rich_first_and_plants_fix_it() {
    use crate::power::{demand_mw, PowerPlantKind};
    let mut w = World::new(WorldConfig::default());
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
    assert!(w.buildings[plant].plant == Some(PowerPlantKind::Nuclear) && w.buildings[plant].jobs == 60);
    assert!(w.economy.treasury < 200_000.0);
    let p = &w.economy.power;
    assert_eq!(p.plants, 1);
    assert!(p.served >= 0.999 && p.unpowered_buildings == 0, "{p:?}");
    assert!(w.buildings.values().all(|b| b.powered >= 0.999));
    // Weather only matters to solar and wind.
    let noon = 0.5;
    assert_eq!(PowerPlantKind::Nuclear.availability(&w.weather, noon), 1.0);
    let mut clear = w.weather.clone();
    clear.cloud = 0.0;
    let mut dull = clear.clone();
    dull.cloud = 1.0;
    assert!(PowerPlantKind::Solar.availability(&clear, noon) > PowerPlantKind::Solar.availability(&dull, noon));
    assert_eq!(PowerPlantKind::Solar.availability(&clear, 0.0), 0.0, "no sun at midnight");
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
    let mut w = World::new(WorldConfig::default());
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
    w.set_gesture(road, Gesture::Road { points: vec![Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], lanes_fwd: 3, lanes_bwd: 3, curved: false, kind: RoadKind::Street }, true);
    w.implement();
    assert!(!w.buildings.contains_key(fronting), "the building fronting the widened road is gone");
    assert!(w.buildings.contains_key(far), "a building on an untouched lot stays");
    assert!(w.lots.values().all(|l| l.building != Some(fronting)));
    let events = w.drain_events();
    assert!(events.iter().any(|e| matches!(e, SimEvent::Expropriated(b, _) if *b == fronting)), "an expropriation event was raised: {events:?}");
    assert!(events.iter().any(|e| matches!(e, SimEvent::BuildingRemoved(b, _) if *b == fronting)));
    let c = &w.citizens[resident];
    assert_eq!(c.home, None);
    assert!(c.memories.iter().any(|m| m.text.contains("pulled down for the road works")), "{:?}", c.memories);
    // The ground is still there, as a smaller lot, and can be built on again.
    let new_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
    assert!(outlines_differ(&old_outline, &w.lots[new_lot].polygon, EXPROPRIATE_AREA_TOL, EXPROPRIATE_DIST_TOL));

    // A footpath drawn through a built lot removes that building too.
    let far_center = w.lots[far_lot].center;
    w.add_path(&[Vec2::new(far_center.x, 380.0), Vec2::new(far_center.x, 620.0)]);
    assert!(!w.buildings.contains_key(far), "a path through the lot takes the building");
    assert!(w.lots.values().all(|l| l.building != Some(far)));
}

#[test]
fn unchanged_parcels_keep_their_buildings_through_unrelated_works() {
    // A new road elsewhere must not shake buildings off parcels that did
    // not change: the outline comparison has to tolerate regeneration.
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
    w.economy.treasury = 500_000.0;
    w.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(1000.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(150.0, 10.0), Vec2::new(330.0, 10.0), Vec2::new(330.0, 90.0), Vec2::new(150.0, 90.0)], LandUse::Commercial);
    w.add_zone(vec![Vec2::new(650.0, 10.0), Vec2::new(830.0, 10.0), Vec2::new(830.0, 90.0), Vec2::new(650.0, 90.0)], LandUse::Commercial);
    // A shop stands where the station would go: the placement is refused.
    let shop_lot = lot_near(&w, Vec2::new(240.0, 25.0), LandUse::Commercial);
    w.build_on(shop_lot);
    let shop = w.lots[shop_lot].building.unwrap();
    let shop_center = w.lots[shop_lot].center;
    let err = w.place_rail_facility(RailFacility::Station, shop_center).expect_err("a station cannot go over a shop");
    assert!(err.contains("built"), "{err}");
    assert!(w.buildings.contains_key(shop) && w.facilities.is_empty());
    let lots_before = w.lots.len();
    // A yard over vacant zoned lots takes the ground; the lots under and
    // across its footprint give way.
    let yard = w.place_rail_facility(RailFacility::FreightYard, Vec2::new(740.0, 40.0)).expect("yard");
    assert!(w.lots.len() < lots_before, "zoned lots under the yard gave way");
    let fp = w.facilities.values().next().unwrap().footprint();
    let yard_lot = w.buildings[yard].lot;
    for (id, l) in w.lots.iter() {
        if id == yard_lot {
            continue;
        }
        assert!(!point_in_polygon(l.center, &fp), "a lot centre under the yard: {:?}", l.center);
        assert!(!polygons_overlap(&fp, &l.polygon), "a lot crossing the yard: {:?}", l.polygon);
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
    for (id, b) in w.buildings.iter() {
        if id == yard {
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
    let mut w = World::new(WorldConfig::default());
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
    // The old tenants cannot pay the new rent and move on.
    let rent = w.buildings[bid].rent;
    let priced_out: Vec<CitizenId> = w.buildings[bid].residents.iter().copied().filter(|&c| w.citizens[c].income * 0.5 < rent).collect();
    assert!(!priced_out.is_empty(), "somebody is priced out by {rent:.0}/day");
    for _ in 0..5 * 1440 {
        w.land_value.fill(0.95);
        fed(&mut w);
        w.tick(&mut minds);
    }
    assert!(w.citizens.values().all(|c| c.hungry_days == 0), "nobody is starving: what follows is about rent");
    for cid in &priced_out {
        assert!(w.citizens.get(*cid).map_or(true, |c| c.home != Some(bid)), "a priced-out tenant is still there");
    }
    assert!(w.economy.displaced_total + w.economy.emigrated_total > 0);
}

#[test]
fn heights_follow_density() {
    let mut w = World::new(WorldConfig::default());
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
    let mut p = World::new(WorldConfig::default());
    p.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    p.add_zone(square(10.0, 10.0, 100.0), LandUse::Park);
    let park = lot_near(&p, Vec2::new(60.0, 20.0), LandUse::Park);
    p.build_on(park);
    assert_eq!(p.buildings[p.lots[park].building.unwrap()].floors, 0);
}

#[test]
fn buildings_go_up_over_days_and_people_move_in_after() {
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
    let ind: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial).map(|(id, _)| id).collect();
    let big = ind.iter().copied().max_by(|a, b| w.lots[*a].area().total_cmp(&w.lots[*b].area())).unwrap();
    let plant = w.build_power(big, PowerPlantKind::Coal).expect("coal plant");
    let b = &w.buildings[plant];
    assert!(b.construction.is_none(), "specials are placed finished");
    assert_eq!(b.jobs, PowerPlantKind::Coal.jobs());
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
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
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
    let queued0 = w.buildings[yard].rail_inbound.len();
    let site_lot = lot_near(&w, Vec2::new(700.0, 40.0), LandUse::Commercial);
    w.build_on(site_lot);
    assert_eq!(w.buildings[yard].rail_inbound.len(), queued0, "construction parts came by truck");
    // A shop's order does, and it is stranded when the branch goes.
    w.import(shop, Resource::Parts, 10.0, 100.0);
    assert_eq!(w.buildings[yard].rail_inbound.len(), queued0 + 1);
    let stock0 = w.buildings[shop].business.as_ref().unwrap().stock_of(Resource::Parts);
    w.start_project();
    w.remove_gesture(branch);
    w.implement();
    assert!(!w.buildings[yard].rail.as_ref().unwrap().works());
    let mut minds = IdleMind;
    for _ in 0..60 {
        w.tick(&mut minds);
    }
    assert!(w.buildings[yard].rail_inbound.is_empty(), "the stranded order left the yard within the hour");
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
    assert_eq!(w.buildings[yard].rail_inbound.len(), 1);
    w.rail.last_freight_tick = 0;
    w.tick(&mut minds);
    assert!(w.buildings[yard].rail_inbound.is_empty(), "two headways without a freight train and the goods go by road");
}

#[test]
fn a_short_commute_at_eight_arrives_before_nine() {
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
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
    let ind = lot_near(&w, Vec2::new(-350.0, 20.0), LandUse::Residential);
    w.lots[ind].land_use = LandUse::Industrial;
    w.build_power(ind, PowerPlantKind::Solar).expect("plant");
    assert!(w.economy.ledger.today_of(Flow::Construction) < 0.0);
    let booked: f32 = (0..9).map(|i| w.economy.ledger.today[i] - today0[i]).sum();
    assert!((w.economy.treasury - before - booked).abs() < 0.01, "treasury moved {} but the books say {}", w.economy.treasury - before, booked);
}

#[test]
fn same_seed_and_script_give_identical_saves() {
    fn run() -> String {
        let mut w = World::new(WorldConfig { seed: 11, ..WorldConfig::default() });
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
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
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
    assert!(!w.buildings.contains_key(shop), "then the lot is cleared");
    assert!(w.lots[shop_lot].building.is_none() || !w.buildings.contains_key(w.lots[shop_lot].building.unwrap()));
}

#[test]
fn four_hungry_days_and_a_citizen_leaves() {
    let mut w = World::new(WorldConfig::default());
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
    assert!(w.drain_events().iter().any(|e| matches!(e, SimEvent::CitizenLeft(x) if *x == c)));
}

#[test]
fn job_search_fills_open_jobs_with_the_fixed_skill_formula() {
    let mut w = World::new(WorldConfig::default());
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
    let mut w = World::new(WorldConfig::default());
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
