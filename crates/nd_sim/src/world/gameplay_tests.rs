//! Tests of the player's tow order (the wheel clamp), passengers riding
//! along in cars, and ferries to towns off the map.

use super::*;
use crate::enforcement::*;
use crate::landscape::{LandscapeParams, LandscapePreset, MapEdge};
use crate::mind::IdleMind;
use crate::ports::{PortKind, ShipKind};
use crate::rides::RideKind;
use crate::vehicle::{CarEvent, CarPlace, ParkKind, VehicleId};

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

fn finish_sites(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

/// Shops west of a cross street, homes east, a long empty street south.
fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, landscape: LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    run(&mut w, 12 * 60);
    finish_sites(&mut w);
    run(&mut w, 60);
    w.economy.treasury = 500_000.0;
    w
}

/// Someone at home with a car parked on the empty south street.
fn owner_parked_south(w: &mut World) -> (CitizenId, VehicleId) {
    let id = w.citizens.iter().filter(|(_, c)| c.home.is_some() && c.can_work()).map(|(id, _)| id).next().expect("someone moved in");
    if let Some(v) = w.citizens[id].car {
        w.drop_car(v, CarEvent::Sold);
    }
    let home = w.citizens[id].home.unwrap();
    {
        let c = &mut w.citizens[id];
        c.activity = Activity::AtHome;
        c.inside = Some(home);
        c.next_decision_tick = u64::MAX;
    }
    let vid = w.give_car_parked(id, Vec2::new(0.0, -240.0));
    assert_eq!(w.vehicles.cars[vid].spot().unwrap().kind, ParkKind::Kerb);
    (id, vid)
}

fn assert_no_car_lost(w: &World) {
    let v = &w.vehicles;
    assert_eq!(v.cars.len() as i64, v.stats.created_total as i64 - v.stats.destroyed_total as i64, "{:?}", v.stats.events_total);
}

// ── the clamp ─────────────────────────────────────────────────────────────

#[test]
fn a_parked_car_ordered_towed_is_clamped_then_towed_and_the_owner_pays() {
    let mut w = town();
    let (id, vid) = owner_parked_south(&mut w);
    let mood = w.citizens[id].mood;
    let job = w.order_tow(vid).expect("a kerbside car can be clamped");
    let spot = *w.vehicles.cars[vid].spot().unwrap();
    assert!(spot.booted, "the clamp is on");
    assert_eq!(w.enforcement.job(job).unwrap().reason, TowReason::Ordered);
    assert!(w.citizens[id].mood < mood, "the owner is not pleased");
    assert!(w.citizens[id].memories.iter().any(|m| m.text.contains("clamped")));
    assert!(w.car_ready(id, spot.pos).is_none(), "a clamped car cannot be driven");
    assert!(w.order_tow(vid).is_ok(), "ordering again changes nothing");
    assert_eq!(w.enforcement.jobs.iter().filter(|j| j.car == vid).count(), 1);
    // No tow company in town: one comes from out of town, and the car ends
    // in its pound, clamp off.
    for _ in 0..1500 {
        run(&mut w, 1);
        if w.vehicles.cars[vid].spot().is_some_and(|s| s.kind == ParkKind::Pound) {
            break;
        }
    }
    let spot = *w.vehicles.cars[vid].spot().expect("impounded");
    assert_eq!(spot.kind, ParkKind::Pound);
    assert!(!spot.booted);
    assert_eq!(w.enforcement.stats.boots_total, 1);
    // The owner pays to get it back.
    if let Some(h) = w.citizens[id].household {
        w.households[h].money = 1000.0;
    }
    w.citizens[id].next_decision_tick = 0;
    for _ in 0..3000 {
        run(&mut w, 1);
        if w.enforcement.stats.released_total > 0 {
            break;
        }
    }
    assert!(w.enforcement.stats.released_total >= 1, "fetched for the fee");
    assert!(w.citizens[id].memories.iter().any(|m| m.text.contains("pound")));
    assert_no_car_lost(&w);
}

#[test]
fn a_car_on_the_road_pulls_over_and_its_people_walk_on() {
    let mut w = town();
    let (id, vid) = owner_parked_south(&mut w);
    let shop = w.buildings.iter().find(|(_, b)| b.land_use == LandUse::Commercial).map(|(id, _)| id).expect("a shop");
    assert!(w.start_drive(id, vid, shop, Purpose::Home), "drives off");
    run(&mut w, 3);
    let CarPlace::Driving(car) = w.vehicles.cars[vid].place else { panic!("on the road") };
    // Not the city's to clamp: a lorry or a car passing through.
    if let Some((through, _)) = w.traffic.cars.iter().find(|(_, c)| c.owned.is_none()) {
        assert!(w.order_tow_car(through).is_err());
    }
    let job = w.order_tow_car(car).expect("clamped where it is");
    assert!(!w.traffic.cars.contains_key(car), "off the lanes: traffic passes");
    let spot = *w.vehicles.cars[vid].spot().expect("parked at the kerb");
    assert!(spot.booted && spot.kind.on_street());
    assert!(matches!(w.citizens[id].activity, Activity::Traveling { to: Some(b), .. } if b == shop) || w.citizens[id].inside == Some(shop), "the driver walks on");
    assert!(w.enforcement.job(job).is_some());
    // It stays put until the truck takes it.
    for _ in 0..30 {
        run(&mut w, 1);
        let s = w.vehicles.cars[vid].spot();
        assert!(s.is_some(), "never driven while clamped");
        if s.is_some_and(|s| s.kind == ParkKind::Hooked) {
            break;
        }
    }
    assert_no_car_lost(&w);
}

#[test]
fn only_kerbside_and_road_cars_can_be_clamped() {
    let mut w = town();
    let (id, vid) = owner_parked_south(&mut w);
    let home = w.citizens[id].home.unwrap();
    w.unpark(vid);
    let spot = w.building_spot(ParkKind::Home, home).unwrap();
    w.park_at(vid, spot);
    assert!(w.order_tow(vid).unwrap_err().contains("off the street"));
}

// ── passengers ────────────────────────────────────────────────────────────

/// Two people of one household at a shop, the car outside.
fn pair_at_shop(w: &mut World) -> (CitizenId, CitizenId, VehicleId, BuildingId, BuildingId) {
    let shop = w.buildings.iter().find(|(_, b)| b.land_use == LandUse::Commercial).map(|(id, _)| id).expect("a shop");
    let mut ids = w.citizens.iter().filter(|(_, c)| c.home.is_some() && c.can_work()).map(|(id, _)| id);
    let (a, b) = (ids.next().unwrap(), ids.next().unwrap());
    let home = w.citizens[a].home.unwrap();
    // One household, one home.
    let hh = w.citizens[a].household;
    w.citizens[b].household = hh;
    w.citizens[b].home = Some(home);
    for x in [a, b] {
        if let Some(v) = w.citizens[x].car {
            w.drop_car(v, CarEvent::Sold);
        }
        let c = &mut w.citizens[x];
        c.activity = Activity::Visiting(shop);
        c.inside = Some(shop);
        c.next_decision_tick = u64::MAX;
    }
    let door = w.building_entrance(shop).unwrap();
    let vid = w.give_car_parked(a, door);
    (a, b, vid, shop, home)
}

/// The driver walks out of `from` to the parked car to drive to `to`.
fn set_off(w: &mut World, id: CitizenId, vid: VehicleId, from: BuildingId, to: BuildingId, purpose: Purpose) {
    let at = w.vehicles.cars[vid].spot().unwrap().pos;
    let door = w.building_entrance(from).unwrap();
    w.walk_to_car(id, vid, at, to, purpose, door);
    assert!(w.citizens[id].to_car.is_some(), "walking to the car");
}

#[test]
fn a_household_rides_home_together_and_both_get_out() {
    let mut w = town();
    let (a, b, vid, shop, home) = pair_at_shop(&mut w);
    set_off(&mut w, a, vid, shop, home, Purpose::Home);
    assert!(w.join_ride(b, a, home, Purpose::Home, RideKind::Outing, true), "the other rides along");
    assert!(w.citizens[b].is_visible(), "walking to the car beside the driver");
    assert!(matches!(w.citizens[b].activity, Activity::Passenger { driver, .. } if driver == a));
    assert!(w.citizens[b].activity.is_traveling());
    let mut seen_inside = false;
    for _ in 0..600 {
        run(&mut w, 1);
        if let CarPlace::Driving(car) = w.vehicles.cars[vid].place {
            if w.car_occupants(car).contains(&b) {
                seen_inside = true;
                assert!(!w.citizens[b].is_visible(), "hidden in the car");
                let pos = w.traffic.cars[car].pos;
                assert!(w.citizens[b].pos.distance(pos) < 0.5, "carried with it");
            }
        }
        if w.citizens[a].inside == Some(home) && w.citizens[b].inside == Some(home) {
            break;
        }
    }
    assert!(seen_inside, "listed among the car's occupants");
    assert_eq!(w.citizens[a].inside, Some(home), "the driver got home");
    assert_eq!(w.citizens[b].inside, Some(home), "and the passenger with them");
    assert!(w.rides.stats.passengers_today >= 1);
    assert!(w.rides.stats.fullest_car >= 2);
}

#[test]
fn a_full_car_takes_no_more_and_nobody_rides_without_a_driver_setting_off() {
    let mut w = town();
    let (a, b, vid, shop, home) = pair_at_shop(&mut w);
    // The driver is not going anywhere yet.
    assert!(!w.join_ride(b, a, home, Purpose::Home, RideKind::Lift, true));
    // A scooter takes one passenger.
    w.vehicles.cars[vid].model.body = crate::vehicle::BodyType::Scooter;
    set_off(&mut w, a, vid, shop, home, Purpose::Home);
    assert!(w.join_ride(b, a, home, Purpose::Home, RideKind::Lift, true));
    let third = w.citizens.iter().filter(|(id, c)| *id != a && *id != b && c.home.is_some()).map(|(id, _)| id).next().unwrap();
    {
        let shop = w.citizens[a].pos;
        let c = &mut w.citizens[third];
        c.activity = Activity::Idle;
        c.inside = None;
        c.pos = shop;
    }
    assert!(!w.join_ride(third, a, home, Purpose::Home, RideKind::Lift, true), "no seat left on a scooter");
}

#[test]
fn a_parent_drops_a_child_on_the_way_and_drives_on() {
    let mut w = town();
    let (parent, kid, vid, shop, home) = pair_at_shop(&mut w);
    // Drop the child at another home on the way, then on home.
    let stop = w.buildings.iter().filter(|(id, b)| b.land_use == LandUse::Residential && *id != home).map(|(id, _)| id).next().expect("another home");
    set_off(&mut w, parent, vid, shop, stop, Purpose::Visit(LandUse::Residential));
    w.rides.onward.push(crate::rides::OnwardLeg { driver: parent, stop, to: home, purpose: Purpose::Home });
    assert!(w.join_ride(kid, parent, stop, Purpose::Home, RideKind::SchoolRun, true));
    for _ in 0..900 {
        run(&mut w, 1);
        if w.citizens[parent].inside == Some(home) && w.citizens[kid].inside == Some(stop) {
            break;
        }
    }
    assert_eq!(w.citizens[kid].inside, Some(stop), "dropped at the stop");
    assert_eq!(w.citizens[parent].inside, Some(home), "the parent drove on home");
    assert!(w.rides.onward.is_empty());
    assert_no_car_lost(&w);
}

// ── ferries ───────────────────────────────────────────────────────────────

fn valley(seed: u64) -> World {
    World::new(WorldConfig { emergencies: false, landscape: LandscapeParams::preset(LandscapePreset::RiverValley, seed), ..WorldConfig::default() })
}

/// A point on the main river inside the basin, its flow direction, the unit
/// vector from it towards the middle of the map, and its width.
fn river_spot(w: &World) -> (Vec2, Vec2, Vec2, f32) {
    let r = &w.landscape.rivers[0];
    let p = *r.points.iter().min_by(|a, b| a.y.abs().total_cmp(&b.y.abs())).unwrap();
    let (seg, _, _) = r.nearest(p);
    let flow = r.flow(seg);
    let mut n = Vec2::new(-flow.y, flow.x);
    if n.dot(-p) < 0.0 {
        n = -n;
    }
    (p, flow, n, r.width)
}

/// A lone terminal on the river with streets, homes and shops behind it.
fn ferry_town() -> (World, BuildingId) {
    let mut w = valley(8);
    w.economy.treasury = 500_000.0;
    let (p, flow, n, width) = river_spot(&w);
    let road_at = p + n * (width * 0.5 + 110.0);
    w.add_road(&[road_at - flow * 300.0, road_at + flow * 300.0], 1, 1);
    w.add_zone(vec![road_at + n * 10.0 - flow * 280.0, road_at + n * 10.0 - flow * 20.0, road_at + n * 90.0 - flow * 20.0, road_at + n * 90.0 - flow * 280.0], LandUse::Residential);
    w.add_zone(vec![road_at + n * 10.0 + flow * 20.0, road_at + n * 10.0 + flow * 280.0, road_at + n * 90.0 + flow * 280.0, road_at + n * 90.0 + flow * 20.0], LandUse::Commercial);
    let click = p + n * (width * 0.5 + 20.0);
    let id = w.place_port(PortKind::FerryTerminal, click).expect("a terminal on the river bank");
    let [s0, _] = w.ports.port_of(id).unwrap().road_line();
    w.add_road(&[s0, road_at - flow * 300.0], 1, 1);
    w.refresh_port_links();
    (w, id)
}

#[test]
fn a_lone_terminal_gets_towns_beyond_the_edges_its_water_reaches() {
    let (w, id) = ferry_town();
    let port = w.ports.port_of(id).unwrap().clone();
    assert!(port.works(), "connected");
    assert!((1..=2).contains(&port.remote.len()), "one or two towns off the map: {:?}", port.remote);
    let edges: Vec<MapEdge> = w.landscape.water_routes_out(port.berth).into_iter().map(|(e, _)| e).collect();
    for town in &port.remote {
        assert!(edges.contains(&town.edge), "{} lies beyond an edge the river reaches ({:?})", town.name, edges);
    }
    assert_eq!(w.ports.remote_terminals(), vec![id], "one terminal is a way in by water");
    assert_eq!(port.remote, crate::ports::remote_terminals(port.berth, &edges), "the same dock always gets the same towns");
}

#[test]
fn coasts_only_sail_out_across_their_own_edge() {
    let land = crate::landscape::Landscape::new(&LandscapeParams::preset(LandscapePreset::Coast, 3));
    let Some(sea) = land.sea.as_ref() else { return };
    // Points out in the sea: each leaves by one edge only, the one its
    // sea opens onto.
    let pts: Vec<Vec2> = sea.outline.iter().copied().filter(|p| sea.contains(*p * 0.98)).map(|p| p * 0.98).take(6).collect();
    for p in pts {
        let out = land.water_routes_out(p);
        assert_eq!(out.len(), 1, "one way out from the sea at {p:?}");
        assert_eq!(out[0].0, MapEdge::of(sea.exit_from(p)));
    }
    // And a lake leads nowhere.
    for lake in &land.lakes {
        if land.rivers.iter().all(|r| r.nearest(lake.center).2 > r.width + 100.0) {
            assert!(land.water_routes_out(lake.center).is_empty());
        }
    }
}

#[test]
fn ferries_sail_out_over_the_edge_and_bring_visitors_and_commuters_back() {
    let (mut w, id) = ferry_town();
    run(&mut w, 6 * 60);
    finish_sites(&mut w);
    run(&mut w, 30);
    w.refresh_attractions();
    assert!(w.tourism.ways_in[2], "the ferry is a way in for visitors");
    let towns = w.ferry_towns(id);
    // A visitor booked on the boat in.
    let hotel = w.buildings.keys().next().unwrap();
    let tid = w.tourism.tourists.insert(crate::tourism::Tourist {
        seed: 5,
        age: 30,
        class: Tier::Mid,
        pos: Vec2::ZERO,
        prev_pos: Vec2::ZERO,
        heading: 0.0,
        prev_heading: 0.0,
        hotel,
        via: crate::tourism::Arrival::Ferry,
        port: Some(id),
        arrived_tick: w.clock.tick,
        leave_day: w.clock.day() + 2,
        money: 500.0,
        spent: 0.0,
        state: crate::tourism::TouristState::Aboard { terminal: id, since: w.clock.tick },
        next_tick: w.clock.tick + crate::tourism::ABOARD_GIVE_UP,
        sights: 0,
        commuter: None,
    });
    let mut out_run = false;
    for _ in 0..(36 * 60) {
        run(&mut w, 1);
        for s in w.ports.ships.values().filter(|s| s.kind == ShipKind::Ferry && s.port == id) {
            if let Some(k) = s.remote {
                let far = s.path.iter().copied().max_by(|a, b| a.x.abs().max(a.y.abs()).total_cmp(&b.x.abs().max(b.y.abs()))).unwrap();
                assert!(far.x.abs().max(far.y.abs()) > crate::landscape::MAP_EDGE + 200.0, "out over the edge");
                assert_eq!(MapEdge::of(far), towns[k as usize].edge, "across the town's own edge");
                out_run = true;
            }
        }
        if out_run && !matches!(w.tourism.tourists.get(tid).map(|t| &t.state), Some(crate::tourism::TouristState::Aboard { .. })) {
            break;
        }
    }
    assert!(out_run, "a ferry ran to a town off the map");
    let t = w.tourism.tourists.get(tid).expect("still in town");
    assert!(!matches!(t.state, crate::tourism::TouristState::Aboard { .. }), "stepped off the boat");
    assert!(w.ports.stats.visitors_in_today >= 1 || w.ports.stats.ferry_riders_today >= 1);
}

#[test]
fn commuters_off_the_morning_boat_work_a_shift_and_go_home() {
    let (mut w, id) = ferry_town();
    run(&mut w, 6 * 60);
    finish_sites(&mut w);
    // Short-staffed businesses by the pier.
    let shops: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.business.is_some() && b.jobs > 0).map(|(id, _)| id).collect();
    assert!(!shops.is_empty(), "businesses near the terminal");
    for &s in &shops {
        w.buildings[s].workers.clear();
    }
    while w.clock.hour() != 7 {
        run(&mut w, 1);
    }
    let before = w.tourism.commuters();
    w.ferry_returned(id, 0);
    assert!(w.tourism.commuters() > before, "commuters came off the boat");
    assert_eq!(w.tourism.count(), 0, "commuters are not visitors");
    let cid = w.tourism.tourists.iter().find(|(_, t)| t.commuter.is_some()).map(|(id, _)| id).unwrap();
    let work = w.tourism.tourists[cid].commuter.unwrap();
    let mut worked = false;
    for _ in 0..(14 * 60) {
        run(&mut w, 1);
        if w.ferry_staff().get(&work).copied().unwrap_or(0) > 0 {
            worked = true;
        }
        if !w.tourism.tourists.contains_key(cid) {
            break;
        }
    }
    assert!(worked, "counted as staff while at work");
    assert!(!w.tourism.tourists.contains_key(cid), "back to the pier after the shift");
    assert!(w.ports.stats.commuters_out_today >= 1 || w.ports.boarding.values().sum::<u32>() >= 1);
}

#[test]
fn a_resident_takes_a_day_trip_out_and_comes_home() {
    let (mut w, id) = ferry_town();
    run(&mut w, 6 * 60);
    finish_sites(&mut w);
    run(&mut w, 30);
    // Nobody can move in by road here: a resident borrowed from another town.
    let home = w.buildings.iter().find(|(_, b)| b.land_use == LandUse::Residential).map(|(id, _)| id).expect("a home by the river");
    let mut person = town().citizens.values().next().expect("someone").clone();
    person.home = Some(home);
    person.work = None;
    person.household = None;
    person.car = None;
    person.owns_car = false;
    person.owns_bike = false;
    person.age = 40;
    person.places = Default::default();
    person.memories.clear();
    person.pos = w.building_entrance(home).unwrap();
    let cid = w.citizens.insert(person);
    {
        let c = &mut w.citizens[cid];
        c.activity = Activity::AtHome;
        c.inside = Some(home);
        c.savings = 200.0;
        c.next_decision_tick = u64::MAX;
    }
    let lu = w.buildings[id].land_use;
    w.travel_to(cid, Some(id), Purpose::Visit(lu));
    let tick = w.clock.tick;
    w.ports.trippers.push(crate::ports::DayTrip { citizen: cid, terminal: id, town: 0, since: tick });
    let mut abroad = false;
    for _ in 0..(30 * 60) {
        run(&mut w, 1);
        w.citizens[cid].next_decision_tick = u64::MAX;
        if matches!(w.citizens[cid].activity, Activity::Abroad { .. }) {
            abroad = true;
            assert!(!w.citizens[cid].is_visible(), "away, out of sight");
        }
        if abroad && w.citizens[cid].inside == Some(home) {
            break;
        }
    }
    assert!(abroad, "boarded the ferry for the day");
    assert_eq!(w.citizens[cid].inside, Some(home), "and came home after");
    assert!(w.citizens[cid].memories.iter().any(|m| m.text.contains("ferry")));
}

#[test]
fn ferry_terminals_from_old_saves_get_their_towns() {
    let (mut w, id) = ferry_town();
    for p in w.ports.ports.values_mut() {
        p.remote.clear();
    }
    let text = w.to_ron().unwrap().replace(",remote:[]", "");
    let mut back = World::from_ron(&text).unwrap();
    back.refresh_port_links();
    assert!(!back.ferry_towns(id).is_empty());
    let _ = &mut w;
}
