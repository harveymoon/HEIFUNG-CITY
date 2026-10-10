//! [pt-transit] Bus lines that always get buses (or say why not), buses
//! that keep to the line, spaced departures, the lines' books, the railway
//! out of town from a single station, and metro riders on the platform.

use super::*;
use crate::mind::IdleMind;
use crate::transit::{self, LineProblem};

/// A block of streets (two east-west, two north-south) and an industrial
/// strip south of it for a depot.
fn block_town() -> World {
    let mut w = World::new(WorldConfig::test_town());
    w.economy.treasury = 500_000.0;
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-400.0, 140.0), Vec2::new(400.0, 140.0)], 1, 1);
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(-300.0, 140.0)], 1, 1);
    w.add_road(&[Vec2::new(300.0, 0.0), Vec2::new(300.0, 140.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-100.0, -150.0), Vec2::new(100.0, -150.0), Vec2::new(100.0, -10.0), Vec2::new(-100.0, -10.0)], LandUse::Industrial);
    w
}

/// The clockwise loop round the block, stops on the kerb it drives along.
fn block_line(w: &mut World) -> RouteId {
    let route = w.add_bus_route();
    for p in [Vec2::new(-200.0, 4.0), Vec2::new(200.0, 4.0), Vec2::new(296.0, 70.0), Vec2::new(200.0, 136.0), Vec2::new(-200.0, 136.0), Vec2::new(-296.0, 70.0)] {
        w.add_bus_stop(route, p).expect("stop snaps to the street");
    }
    route
}

fn depot(w: &mut World) -> BuildingId {
    let lot = w.lots.iter().find(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Industrial && l.area() >= transit::DEPOT_MIN_AREA).map(|(id, _)| id).expect("industrial lot");
    w.build_depot(lot).expect("depot built")
}

fn buses_on(w: &World, route: RouteId) -> Vec<CarId> {
    w.traffic.cars.iter().filter(|(_, c)| c.bus.as_ref().is_some_and(|b| b.route == route)).map(|(id, _)| id).collect()
}

/// Run to `minute` of the day.
fn run_to(w: &mut World, minute: u32) {
    let mut minds = IdleMind;
    while w.clock.minute_of_day() as u32 != minute {
        w.tick(&mut minds);
    }
}

#[test]
fn a_line_no_depot_can_reach_says_so_and_does_not_hold_up_the_others() {
    let mut w = block_town();
    // An island street far off, joined to nothing: its line can never be
    // reached from the depot. It is made first, so the dispatcher meets it
    // first (it used to try only the first line short of buses, and with
    // that one unreachable, no line ever got a bus: F3, F4).
    w.add_road(&[Vec2::new(-400.0, 600.0), Vec2::new(400.0, 600.0)], 1, 1);
    let island = w.add_bus_route();
    w.add_bus_stop(island, Vec2::new(-200.0, 604.0)).unwrap();
    w.add_bus_stop(island, Vec2::new(200.0, 604.0)).unwrap();
    let line = block_line(&mut w);
    // No depot yet: both lines say so.
    assert!(w.line_problems(line).contains(&LineProblem::NoDepot), "{:?}", w.line_problems(line));
    depot(&mut w);
    run_to(&mut w, 8 * 60);
    let mut minds = IdleMind;
    for _ in 0..30 {
        w.tick(&mut minds);
    }
    assert!(!buses_on(&w, line).is_empty(), "the reachable line got its bus");
    assert!(buses_on(&w, island).is_empty());
    let problems = w.line_problems(island);
    assert!(problems.contains(&LineProblem::DepotUnreachable), "the island line says why: {problems:?}");
    assert!(problems.iter().any(|p| p.blocks()) && problems.iter().all(|p| !p.to_string().is_empty()));
    assert!(!w.line_problems(line).iter().any(|p| p.blocks()), "the running line is fine: {:?}", w.line_problems(line));
    // At night the line says the buses are home, and when they come out.
    run_to(&mut w, 23 * 60 + 45);
    let night = w.line_problems(line);
    assert!(night.contains(&LineProblem::OutOfService) && night.iter().any(|p| p.to_string().contains("05:30")), "{night:?}");
}

#[test]
fn routes_are_checked_before_they_are_built() {
    let mut w = block_town();
    w.add_road(&[Vec2::new(-400.0, 600.0), Vec2::new(400.0, 600.0)], 1, 1);
    let snap = |w: &World, p: Vec2| w.transit.stop_for_click(&w.roads, p).unwrap();
    let good = [snap(&w, Vec2::new(-200.0, 4.0)), snap(&w, Vec2::new(200.0, 4.0)), snap(&w, Vec2::new(200.0, 136.0))];
    assert!(Transit::check_stops(&w.traffic.net, &good).is_empty(), "{:?}", Transit::check_stops(&w.traffic.net, &good));
    assert_eq!(Transit::check_stops(&w.traffic.net, &good[..1]), vec![LineProblem::TooFewStops]);
    // A stop on the island street: no road to it, and none back.
    let bad = [good[0], good[1], snap(&w, Vec2::new(0.0, 604.0))];
    let problems = Transit::check_stops(&w.traffic.net, &bad);
    assert!(problems.contains(&LineProblem::NoRoad { leg: 1 }) && problems.contains(&LineProblem::NoRoad { leg: 2 }), "{problems:?}");
    assert_eq!(problems.iter().filter_map(|p| p.leg()).collect::<Vec<_>>(), vec![1, 2]);
    // Stops on the far kerb from the way round: each leg has to go all
    // the way round the block to come back past its stop.
    let back = [snap(&w, Vec2::new(-200.0, -4.0)), snap(&w, Vec2::new(-100.0, -4.0)), snap(&w, Vec2::new(0.0, -4.0))];
    let problems = Transit::check_stops(&w.traffic.net, &back);
    assert!(problems.iter().any(|p| matches!(p, LineProblem::LongWayRound { .. })), "{problems:?}");
}

#[test]
fn the_lines_warn_when_the_depots_hold_fewer_buses_than_they_want() {
    let mut w = block_town();
    let a = block_line(&mut w);
    let b = w.add_bus_route();
    for p in [Vec2::new(-200.0, 4.0), Vec2::new(200.0, 4.0)] {
        w.add_bus_stop(b, p).unwrap();
    }
    depot(&mut w);
    w.set_route_buses(a, 8);
    w.set_route_buses(b, 6);
    assert_eq!(w.fleet_balance().0, 14);
    assert!(w.line_problems(a).iter().any(|p| matches!(p, LineProblem::FleetShort { wanted: 14, .. })), "{:?}", w.line_problems(a));
    w.set_route_buses(b, 1);
    assert!(!w.line_problems(a).iter().any(|p| matches!(p, LineProblem::FleetShort { .. })));
}

#[test]
fn buses_leave_the_depot_a_headway_apart_and_keep_to_the_drawn_line() {
    let mut w = block_town();
    let line = block_line(&mut w);
    w.set_route_buses(line, 3);
    depot(&mut w);
    let mut minds = IdleMind;
    let mut seen: Vec<(CarId, u64)> = Vec::new();
    let mut first_stop: std::collections::HashMap<CarId, usize> = std::collections::HashMap::new();
    let mut followed = 0usize;
    let mut departed: std::collections::HashMap<CarId, usize> = std::collections::HashMap::new();
    for _ in 0..(10 * 60) {
        w.tick(&mut minds);
        for id in buses_on(&w, line) {
            if !seen.iter().any(|s| s.0 == id) {
                seen.push((id, w.clock.tick));
                first_stop.insert(id, w.traffic.cars[id].bus.as_ref().unwrap().next_stop);
            }
            // Leaving a stop: the bus drives the leg as drawn.
            if let Some(&(k, _)) = w.transit.departures.get(&id) {
                if departed.insert(id, k) != Some(k) {
                    let car = &w.traffic.cars[id];
                    let drawn = w.transit.routes[line].leg_lanes[k].as_ref().expect("a leg with a road");
                    assert_eq!(car.route, drawn.0, "bus {id:?} leaving stop {k} drives the drawn leg");
                    followed += 1;
                }
            }
        }
    }
    assert_eq!(seen.len(), 3, "three buses out");
    let gap = w.transit.routes[line].release_gap(w.config.traffic_seconds_per_tick);
    assert!(gap > transit::DISPATCH_GAP, "a loop of the block takes a while: {gap}");
    for pair in seen.windows(2) {
        assert!(pair[1].1 - pair[0].1 >= gap - 1, "a headway apart: {seen:?} (gap {gap})");
    }
    // And they joined the loop at stops spread round it.
    let starts: Vec<usize> = seen.iter().filter_map(|(id, _)| first_stop.get(id).copied()).collect();
    assert_eq!(starts, vec![0, 2, 4], "spread round the six stops");
    assert!(followed >= 6, "buses left several stops: {followed}");
}

#[test]
fn a_bus_trip_is_timed_from_its_last_stop() {
    let mut w = block_town();
    let line = block_line(&mut w);
    depot(&mut w);
    run_to(&mut w, 12 * 60);
    let id = buses_on(&w, line)[0];
    let (elapsed, expected) = w.car_trip_minutes(id).expect("timed");
    assert!(elapsed <= expected * 3.0 + 10.0, "a bus out since the morning is timed on its leg: {elapsed} of {expected}");
    let j = w.car_journey(id).unwrap();
    assert!(j.from.label.starts_with("stop "), "from the last stop: {}", j.from.label);
}

#[test]
fn lines_take_stops_in_between_and_share_kerbs() {
    let mut w = block_town();
    let line = block_line(&mut w);
    let at = w.insert_bus_stop(line, Some(1), Vec2::new(0.0, 4.0)).unwrap();
    assert_eq!(at, 1);
    let r = &w.transit.routes[line];
    assert_eq!(r.stops.len(), 7);
    assert!(r.stops[1].pos.x.abs() < 1.0 && r.stops[2].pos.x > 150.0, "between the first and the old second");
    assert!(r.runnable());
    // Another line clicking on that stop shares it.
    let other = w.add_bus_route();
    w.add_bus_stop(other, Vec2::new(3.0, 6.0)).unwrap();
    assert_eq!(w.transit.routes[other].stops[0].pos, w.transit.routes[line].stops[1].pos, "the same kerb");
}

#[test]
fn each_line_keeps_books_and_sets_its_fare() {
    let mut w = block_town();
    let line = block_line(&mut w);
    depot(&mut w);
    w.set_route_fare(line, 99.0).unwrap();
    assert_eq!(w.transit.routes[line].fare, transit::MAX_FARE);
    w.set_route_fare(line, 1.5).unwrap();
    assert!(w.set_route_fare(line, f32::NAN).is_err());
    run_to(&mut w, 0);
    run_to(&mut w, 0);
    let r = &w.transit.routes[line];
    // One bus out the whole service day: about one bus-day of upkeep.
    assert!((r.upkeep_yesterday - transit::BUS_UPKEEP_PER_DAY).abs() < transit::BUS_UPKEEP_PER_DAY * 0.2, "upkeep {}", r.upkeep_yesterday);
    assert_eq!(r.bus_minutes_today, 0, "rolled over");
}

#[test]
fn a_single_station_brings_people_in_from_out_of_town() {
    use crate::rail::RailFacility;
    let mut w = World::new(WorldConfig::test_town());
    w.economy.treasury = 500_000.0;
    w.add_rail(&[Vec2::new(-200.0, TRUNK_Y), Vec2::new(-100.0, -400.0), Vec2::new(-40.0, -300.0), Vec2::new(0.0, -200.0), Vec2::new(0.0, -60.0), Vec2::new(900.0, -60.0)]);
    w.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(1000.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(120.0, 0.0), Vec2::new(120.0, 120.0)], 1, 1);
    w.add_zone(vec![Vec2::new(150.0, 10.0), Vec2::new(500.0, 10.0), Vec2::new(500.0, 90.0), Vec2::new(150.0, 90.0)], LandUse::Commercial);
    let station = w.place_rail_facility(RailFacility::Station, Vec2::new(240.0, -45.0)).expect("station");
    let stub_end = w.facilities.values().find(|f| f.building == Some(station)).unwrap().road_line()[1];
    w.add_road(&[stub_end, Vec2::new(stub_end.x, 0.0)], 1, 1);
    assert!(w.buildings[station].rail.as_ref().unwrap().works());
    assert_eq!(w.stations.len(), 1, "one station: no trip within the town by train");
    // Shops beside it with jobs and nobody to fill them.
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Commercial).map(|(id, _)| id).take(3).collect();
    for lot in lots {
        w.build_on(lot);
        if let Some(b) = w.lots[lot].building {
            w.finish_construction(b);
        }
    }
    let mut minds = IdleMind;
    let mut came = 0;
    for _ in 0..(36 * 60) {
        w.tick(&mut minds);
        came = came.max(w.rail.stats.from_out_of_town_today);
        if came > 0 && w.rail.stats.riders_today > 0 {
            break;
        }
    }
    assert!(came > 0, "commuters came in by train to a town with one station");
    assert!(w.rail.stats.riders_today > 0 && w.rail.stats.fares_today > 0.0, "and are the station's riders");
    assert!(w.tourism.tourists.values().any(|t| t.commuter.is_some() && t.via == crate::tourism::Arrival::Rail), "commuters off the train");
}
