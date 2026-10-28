//! [carlife] Tests of the car lifecycle and the mode choice.

use super::*;
use crate::citizens::TripMode;
use crate::market::Resource;
use crate::mind::IdleMind;
use crate::vehicle::{CarEvent, CarPlace, Errand, ParkKind, VehicleId};

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

fn finish_all(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

/// A main road with shops west of a cross street and homes east of it; a
/// parking lot among the shops. Shopping streets have no kerb parking, the
/// homes' street does.
fn town() -> (World, BuildingId, BuildingId) {
    let mut w = World::new(WorldConfig { emergencies: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Commercial && l.car_access()).map(|(id, _)| id).collect();
    assert!(lots.len() >= 2);
    w.build_on(lots[0]);
    let shop = w.lots[lots[0]].building.unwrap();
    // [malls] A plain shop (its plot may draw a strip mall, with its own car
    // park out front): these tests are about the town's lots and kerbs.
    w.buildings[shop].plot.form = None;
    w.buildings[shop].floors = w.buildings[shop].floors.min(2);
    w.finish_construction(shop);
    let lot = w.build_parking(lots[1]).expect("parking lot opens");
    run(&mut w, 12 * 60);
    finish_all(&mut w);
    run(&mut w, 60);
    (w, shop, lot)
}

/// A grown-up living in the town, at home, with no bike and no car yet.
fn resident(w: &mut World) -> CitizenId {
    let id = w.citizens.iter().find(|(_, c)| c.home.is_some() && c.can_work()).map(|(id, _)| id).expect("someone moved in");
    if let Some(vid) = w.citizens[id].car {
        w.drop_car(vid, CarEvent::Sold);
    }
    // Any car still on the road with them in it is parked first.
    let home = w.citizens[id].home.unwrap();
    let c = &mut w.citizens[id];
    c.owns_bike = false;
    c.activity = Activity::AtHome;
    c.inside = Some(home);
    c.savings = 200.0;
    c.income = 80.0;
    c.next_decision_tick = u64::MAX;
    id
}

/// Drive until the car is parked again (or give up after `limit` ticks).
fn until_parked(w: &mut World, vid: VehicleId, limit: usize) -> Vec<Vec2> {
    let mut seen = Vec::new();
    let mut minds = IdleMind;
    for _ in 0..limit {
        w.tick(&mut minds);
        match w.vehicles.cars[vid].place {
            CarPlace::Driving(cid) => seen.push(w.traffic.cars[cid].pos),
            CarPlace::Parked(_) if !seen.is_empty() => break,
            CarPlace::Parked(_) => {}
        }
    }
    seen
}

/// The registry and the road agree: every record is parked or on the road
/// exactly once, every private car on the road has a record, every owner's
/// car exists, and the count is what the events say.
fn assert_consistent(w: &World) {
    let v = &w.vehicles;
    assert_eq!(v.cars.len() as i64, v.stats.created_total as i64 - v.stats.destroyed_total as i64, "cars = created - destroyed {:?}", v.stats.events_total);
    for (vid, car) in &v.cars {
        if let CarPlace::Driving(cid) = car.place {
            let t = w.traffic.cars.get(cid).unwrap_or_else(|| panic!("car {vid:?} driving a vanished traffic car"));
            assert_eq!(t.owned, Some(vid));
        }
    }
    for (cid, t) in &w.traffic.cars {
        if let Some(vid) = t.owned {
            let rec = v.cars.get(vid).expect("a private car on the road has a record");
            assert_eq!(rec.place, CarPlace::Driving(cid));
        }
    }
    for (id, c) in &w.citizens {
        if let Some(vid) = c.car {
            assert_eq!(v.cars.get(vid).map(|r| r.owner), Some(id), "an owner's car exists");
            assert!(c.owns_car);
        }
    }
}

#[test]
fn a_driver_pays_at_the_lot_parks_there_and_leaves_from_there() {
    let (mut w, shop, lot) = town();
    // No kerb parking anywhere in this town: the lot it has to be.
    w.refresh_no_kerb();
    let all: Vec<SegmentId> = w.roads.alive_segments().map(|(id, _)| id).collect();
    w.vehicles.no_kerb.1.extend(all);
    let c = resident(&mut w);
    let home = w.citizens[c].home.unwrap();
    w.buildings[shop].parking_capacity = 0;
    let at_home = w.building_entrance(home).unwrap();
    let vid = w.give_car_parked(c, at_home);
    assert!(w.citizens[c].owns_car);
    let used0 = w.buildings[lot].parking_used;
    let money0 = w.buildings[lot].business.as_ref().unwrap().money;
    w.travel_to(c, Some(shop), Purpose::Shop(Resource::Goods));
    assert!(matches!(w.citizens[c].activity, Activity::Driving { .. }) || w.citizens[c].to_car.is_some(), "a car owner going across town drives");
    let seen = until_parked(&mut w, vid, 900);
    assert!(!seen.is_empty(), "the car drove");
    let spot = *w.vehicles.cars[vid].spot().expect("parked");
    assert_eq!(spot.kind, ParkKind::Lot, "no kerb parking: the lot it is (open {}, {} m from the shop, free {}, events {:?})", w.lot_open(lot), w.lots[w.buildings[lot].lot].center.distance(w.building_entrance(shop).unwrap()), w.buildings[lot].parking_free(), w.citizens[c].memories.iter().map(|m| &m.text).collect::<Vec<_>>());
    assert_eq!(spot.building, Some(lot));
    // Count the cars in it rather than trust the counter alone.
    let in_lot = |w: &World| w.vehicles.cars.values().filter(|c| c.spot().is_some_and(|s| s.building == Some(lot))).count() as u16;
    assert_eq!(w.buildings[lot].parking_used, in_lot(&w), "the lot's count is the cars in it");
    assert!(in_lot(&w) >= used0 + 1);
    assert!(w.buildings[lot].business.as_ref().unwrap().money > money0, "the lot earned its fee");
    assert!(w.citizens[c].commute.fee > 0.0 || w.citizens[c].commute.mode.is_none(), "the driver paid");
    assert_eq!(w.citizens[c].parked_at, Some(lot));
    // Leaving the shop does not free the space: the car is still there.
    w.citizens[c].inside = Some(shop);
    w.leave_building(c);
    assert_eq!(w.vehicles.cars[vid].spot().map(|s| s.building), Some(Some(lot)), "the space is the car's until it drives off");
    // Home again: the car pulls out of the lot.
    w.citizens[c].activity = Activity::Visiting(shop);
    w.citizens[c].inside = Some(shop);
    w.travel_to(c, Some(home), Purpose::Home);
    let mut first: Option<Vec2> = None;
    for _ in 0..200 {
        run(&mut w, 1);
        if let CarPlace::Driving(cid) = w.vehicles.cars[vid].place {
            first = Some(w.traffic.cars[cid].prev_pos);
            break;
        }
    }
    let first = first.expect("the car left");
    assert_eq!(w.buildings[lot].parking_used, in_lot(&w), "and its space is free");
    let (seg, pt) = w.truck_stop(lot).unwrap();
    let _ = seg;
    assert!(first.distance(pt) < 25.0, "it joined the road at the lot ({first} vs {pt})");
    assert_consistent(&w);
}

#[test]
fn a_car_parks_at_the_kerb_where_it_stops_and_pulls_out_from_there() {
    let (mut w, shop, _lot) = town();
    let c = resident(&mut w);
    let home = w.citizens[c].home.unwrap();
    // The car waits near the shops; its owner is in the shop.
    let near_shop = w.building_entrance(shop).unwrap();
    let vid = w.give_car_parked(c, near_shop);
    w.citizens[c].activity = Activity::Visiting(shop);
    w.citizens[c].inside = Some(shop);
    w.buildings[home].parking_capacity = 0;
    w.travel_to(c, Some(home), Purpose::Home);
    let seen = until_parked(&mut w, vid, 900);
    let last = *seen.last().expect("it drove");
    let spot = *w.vehicles.cars[vid].spot().expect("parked");
    assert!(matches!(spot.kind, ParkKind::Kerb | ParkKind::Overflow), "a home with no spaces: the street ({:?})", spot.kind);
    assert!(spot.pos.distance(last) < 8.0, "parked where it stopped: {} vs {}", spot.pos, last);
    if spot.kind == ParkKind::Kerb {
        let seg = spot.segment.unwrap();
        assert!(w.kerb_allowed(seg));
        assert_eq!(w.vehicles.kerbs[&seg].get(&(spot.side, spot.slot)), Some(&vid), "it holds its slot");
    }
    // Next trip: it pulls out from that very spot.
    let c_pos = w.citizens[c].pos;
    let _ = c_pos;
    w.citizens[c].activity = Activity::AtHome;
    w.citizens[c].inside = Some(home);
    w.travel_to(c, Some(shop), Purpose::Shop(Resource::Goods));
    let mut first = None;
    for _ in 0..200 {
        run(&mut w, 1);
        if let CarPlace::Driving(cid) = w.vehicles.cars[vid].place {
            first = Some(w.traffic.cars[cid].prev_pos);
            break;
        }
    }
    let first = first.expect("it drove off");
    assert!(first.distance(spot.pos) < 4.0, "from its spot ({first} vs {})", spot.pos);
    assert_consistent(&w);
}

#[test]
fn a_jam_never_deletes_a_car_and_the_driver_feels_it() {
    let (mut w, shop, _lot) = town();
    let c = resident(&mut w);
    let home = w.citizens[c].home.unwrap();
    let vid = w.give_car_parked(c, w.building_entrance(home).unwrap());
    let car_pos = w.vehicles.cars[vid].spot().unwrap().pos;
    w.citizens[c].inside = None;
    w.citizens[c].pos = car_pos;
    w.begin_trip(c, TripMode::Car, car_pos.distance(w.building_entrance(shop).unwrap()));
    assert!(w.start_drive(c, vid, shop, Purpose::Shop(Resource::Goods)));
    let CarPlace::Driving(cid) = w.vehicles.cars[vid].place else { panic!("driving") };
    // Stuck fast for many times the old half-hour timeout.
    w.traffic.cars[cid].hold = f32::MAX;
    run(&mut w, 2000);
    assert!(w.traffic.cars.contains_key(cid), "still on the road");
    assert_eq!(w.vehicles.cars[vid].place, CarPlace::Driving(cid));
    assert!(matches!(w.citizens[c].activity, Activity::Driving { car, .. } if car == cid), "still in it");
    assert_consistent(&w);
    // The jam clears: it gets there, parks, and the driver remembers.
    w.traffic.cars[cid].hold = 0.0;
    until_parked(&mut w, vid, 900);
    assert!(w.vehicles.cars[vid].spot().is_some(), "parked at last");
    run(&mut w, 120);
    assert!(w.citizens[c].commute.pain_of(TripMode::Car) > 1.0, "that hurt: {}", w.citizens[c].commute.pain_of(TripMode::Car));
    assert_consistent(&w);
}

#[test]
fn repeated_pain_changes_how_a_citizen_travels() {
    let (mut w, shop, _lot) = town();
    let c = resident(&mut w);
    let home = w.citizens[c].home.unwrap();
    let vid = w.give_car_parked(c, w.building_entrance(home).unwrap());

    let q = super::trips::TripQuery { from: w.building_entrance(home).unwrap(), to: w.building_entrance(shop).unwrap(), target: shop, purpose: Purpose::Shop(Resource::Goods), allow_transit: true, allow_car: true, allow_bike: true };
    let best = |w: &World| w.mode_options(c, &q).into_iter().min_by(|a, b| a.cost.total_cmp(&b.cost)).unwrap().mode;
    let fresh = w.mode_options(c, &q);
    assert!(fresh.iter().any(|o| o.mode == TripMode::Car), "the car is an option");
    assert!(fresh.iter().any(|o| o.mode == TripMode::Walk), "so is walking");
    let first = best(&w);
    assert_eq!(first, TripMode::Car, "with no bad memories, the car: {fresh:?}");
    // Day after day of jams and circling for a space.
    for _ in 0..6 {
        w.citizens[c].commute.feel(TripMode::Car, 3.0);
        w.citizens[c].commute.parking_pain = 2.5;
    }
    let after = best(&w);
    assert_ne!(after, TripMode::Car, "after the misery, something else: {:?}", w.mode_options(c, &q));
    // Habits follow: with a bike to fall back on, the car is sold.
    let gone0 = w.vehicles.stats.destroyed_total;
    // [ledger] The wheels are paid from the household's purse.
    let hid = w.citizens[c].household.expect("a household");
    w.households[hid].money = 0.0;
    w.citizens[c].owns_bike = true;
    for day in 0..40 {
        w.wheels(c, hid, day);
        if w.citizens[c].car.is_none() {
            break;
        }
    }
    assert!(w.citizens[c].car.is_none(), "sold the car");
    assert!(w.households[hid].money > 0.0, "and got money for it");
    assert!(w.citizens[c].memories.iter().any(|m| m.text.contains("Sold the car")), "and remembers why");
    // [carlife] It is the dealer's now, still parked where it was until
    // the dealer's driver comes for it in working hours.
    let car = &w.vehicles.cars[vid];
    let dealer = car.dealer.expect("the dealer's now");
    assert!(car.spot().is_some(), "waits where it was parked to be fetched");
    assert!(!w.citizens.contains_key(car.owner), "nobody's car any more");
    let mut on_road = false;
    for _ in 0..3000 {
        run(&mut w, 1);
        let car = w.vehicles.cars.get(vid).expect("a sold car never vanishes in town");
        if let CarPlace::Driving(cid) = car.place {
            on_road = true;
            assert!(matches!(w.traffic.cars[cid].errand, Errand::Sale(_)), "on its way to the dealer: {:?}", w.traffic.cars[cid].errand);
        }
        if car.spot().is_some_and(|s| s.kind == ParkKind::Dealer) {
            break;
        }
    }
    assert!(on_road, "fetched by the dealer's driver");
    let spot = *w.vehicles.cars[vid].spot().expect("parked on the forecourt");
    assert_eq!((spot.kind, spot.building), (ParkKind::Dealer, Some(dealer)), "on the dealer's forecourt as used stock");
    let lot = w.building_lot(dealer).unwrap();
    assert!(crate::geom::point_in_polygon(spot.pos, &lot.polygon), "inside the dealer's lot, not on the road");
    assert_eq!(w.vehicles.stats.destroyed_total, gone0, "still in the city");
    assert_consistent(&w);
    // Unsold, it goes to the trade out of town a few days later: driven
    // off the map, never deleted on the forecourt.
    // (Or a resident buys it off the forecourt first and it is delivered.)
    let (mut left_by_road, mut resold) = (false, false);
    for _ in 0..(6 * 1440) {
        run(&mut w, 1);
        match w.vehicles.cars.get(vid).map(|c| c.place) {
            Some(CarPlace::Driving(cid)) => match w.traffic.cars[cid].errand {
                Errand::Sale(_) => left_by_road = true,
                Errand::Deliver => {
                    resold = true;
                    break;
                }
                e => panic!("a dealer's car on an errand of its own: {e:?}"),
            },
            Some(CarPlace::Parked(_)) => {}
            None => break,
        }
    }
    assert!(resold || (left_by_road && !w.vehicles.cars.contains_key(vid)), "sold on, or driven out of town to the trade");
    if resold {
        assert!(w.citizens.contains_key(w.vehicles.cars[vid].owner), "its new owner's");
        return;
    }
    assert!(w.vehicles.stats.destroyed_total > gone0);
    assert_consistent(&w);
}

#[test]
fn a_mornings_car_sales_are_fetched_through_the_day_not_all_at_once() {
    let (mut w, _shop, _lot) = town();
    let mut sold = Vec::new();
    for _ in 0..8 {
        let c = resident(&mut w);
        let home = w.citizens[c].home.unwrap();
        let vid = w.give_car_parked(c, w.building_entrance(home).unwrap());
        assert!(w.sell_car(c, "Sold the car.", f32::MAX).is_some());
        sold.push(vid);
    }
    // Fetched over the working day: never more than a few set off in an hour.
    let mut started: Vec<u64> = Vec::new();
    for _ in 0..(2 * 1440) {
        run(&mut w, 1);
        for &vid in &sold {
            if matches!(w.vehicles.cars.get(vid).map(|c| c.place), Some(CarPlace::Driving(_))) && !started.contains(&(slotmap::Key::data(&vid).as_ffi())) {
                started.push(slotmap::Key::data(&vid).as_ffi());
                let h = w.clock.hour();
                assert!((8..19).contains(&h), "fetched in working hours, not at {h}");
            }
        }
    }
    assert_eq!(started.len(), sold.len(), "every sold car was fetched");
    // On the dealer's used-car line, or (the line full) driven on out of
    // town to the trade: gone only at the map edge, counted as sold.
    let sold0 = w.vehicles.stats.events_total.get("sold").copied().unwrap_or(0);
    let mut on_court = 0;
    let mut sold_on = 0;
    let mut gone = 0;
    for &vid in &sold {
        match w.vehicles.cars.get(vid).map(|c| c.place) {
            // (Or sold on off the forecourt meanwhile and delivered to its
            // buyer, a resident whose car it now is.)
            Some(CarPlace::Parked(s)) if s.kind != ParkKind::Dealer => {
                let c = &w.vehicles.cars[vid];
                assert!(c.dealer.is_none() && w.citizens.get(c.owner).is_some_and(|o| o.car == Some(vid)), "a sold car parked off the forecourt is a buyer's: {:?} at {:?}", s.kind, s.pos);
                // (It stood on the forecourt first: the dealer sold it on.)
                sold_on += 1;
            }
            Some(CarPlace::Parked(s)) => {
                assert_eq!(s.kind, ParkKind::Dealer, "on the forecourt");
                on_court += 1;
            }
            Some(CarPlace::Driving(cid)) => assert!(matches!(w.traffic.cars[cid].errand, Errand::Sale(_)), "on its way out to the trade"),
            None => gone += 1,
        }
    }
    assert!(on_court + sold_on >= 1, "the forecourt took some (still there, or sold on to a buyer)");
    assert!(sold0 as usize >= gone, "every car gone left town as sold, none vanished");
    assert_consistent(&w);
}

#[test]
fn a_buyer_takes_a_used_car_off_the_forecourt() {
    let (mut w, _shop, _lot) = town();
    let a = resident(&mut w);
    let home = w.citizens[a].home.unwrap();
    let vid = w.give_car_parked(a, w.building_entrance(home).unwrap());
    w.sell_car(a, "Sold the car.", f32::MAX);
    for _ in 0..(2 * 1440) {
        run(&mut w, 1);
        if w.vehicles.cars[vid].spot().is_some_and(|s| s.kind == ParkKind::Dealer) {
            break;
        }
    }
    assert_eq!(w.vehicles.cars[vid].spot().map(|s| s.kind), Some(ParkKind::Dealer));
    let created = w.vehicles.stats.created_total;
    let b = resident(&mut w);
    let model = w.vehicles.cars[vid].model;
    let paid = w.buy_car(b, model, 300.0).expect("bought");
    assert!(paid <= 300.0);
    assert_eq!(w.citizens[b].car, Some(vid), "the used car off the forecourt");
    assert_eq!(w.vehicles.stats.created_total, created, "no new car made");
    assert!(w.vehicles.cars[vid].dealer.is_none());
    assert_consistent(&w);
}

#[test]
fn long_walks_buy_a_bike_at_a_shop() {
    let (mut w, shop, _lot) = town();
    let c = resident(&mut w);
    for _ in 0..4 {
        w.citizens[c].commute.feel(TripMode::Walk, 2.5);
    }
    let money0 = w.buildings[shop].business.as_ref().map_or(0.0, |b| b.money);
    let hid = w.citizens[c].household.expect("a household");
    w.households[hid].money = 300.0;
    for day in 0..30 {
        w.wheels(c, hid, day);
        if w.citizens[c].owns_bike {
            break;
        }
    }
    assert!(w.citizens[c].owns_bike, "bought a bike");
    assert!(w.households[hid].money < 300.0, "and paid for it");
    assert!(w.citizens[c].memories.iter().any(|m| m.text.contains("Tired of the long walks")), "{:?}", w.citizens[c].memories.iter().map(|m| &m.text).collect::<Vec<_>>());
    assert!(w.buildings[shop].business.as_ref().map_or(0.0, |b| b.money) >= money0, "at the shop");
    assert!(w.vehicles.stats.bikes_bought_today >= 1);
}

#[test]
fn transit_that_fails_buys_a_car_that_is_delivered_home() {
    let (mut w, _shop, _lot) = town();
    let c = resident(&mut w);
    for _ in 0..4 {
        w.transit_failed(c);
    }
    let hid = w.citizens[c].household.expect("a household");
    w.households[hid].money = 2000.0;
    for day in 0..30 {
        w.wheels(c, hid, day);
        if w.citizens[c].car.is_some() {
            break;
        }
    }
    let vid = w.citizens[c].car.expect("bought a car");
    assert!(w.households[hid].money < 2000.0);
    assert_eq!(w.vehicles.stats.events_total.get("bought").copied().unwrap_or(0) + w.vehicles.stats.events_total.get("bought_out_of_town").copied().unwrap_or(0), 1);
    // It is on its way from the dealer (or the map edge), not at the door.
    match w.vehicles.cars[vid].place {
        CarPlace::Driving(cid) => assert_eq!(w.traffic.cars[cid].errand, Errand::Deliver),
        CarPlace::Parked(s) => assert_eq!(s.kind, ParkKind::Dealer, "waiting at the dealer"),
    }
    let home = w.building_entrance(w.citizens[c].home.unwrap()).unwrap();
    until_parked(&mut w, vid, 900);
    let spot = *w.vehicles.cars[vid].spot().expect("delivered and parked");
    assert_ne!(spot.kind, ParkKind::Dealer);
    assert!(spot.pos.distance(home) < 260.0, "near home: {} vs {home}", spot.pos);
    assert_consistent(&w);
}

#[test]
fn parked_cars_survive_save_and_load_and_runs_are_deterministic() {
    let (mut a, _shop, _lot) = town();
    let (mut b, _, _) = town();
    run(&mut a, 6 * 60);
    run(&mut b, 6 * 60);
    assert!(!a.vehicles.cars.is_empty(), "the town has cars");
    let places = |w: &World| w.vehicles.cars.iter().map(|(id, c)| (id, format!("{:?}", c.place))).collect::<Vec<_>>();
    assert_eq!(places(&a), places(&b), "same seed, same moves: the same cars in the same places");
    assert_eq!(a.vehicles.stats.events_total, b.vehicles.stats.events_total);
    assert!(a.vehicles.cars.values().any(|c| c.spot().is_some()), "some are parked");
    let back = World::from_ron(&a.to_ron().unwrap()).unwrap();
    assert_eq!(places(&back), places(&a), "a save keeps every car where it is");
    assert_eq!(back.vehicles.kerbs, a.vehicles.kerbs, "kerb slots rebuilt as they were");
    for (id, bld) in &a.buildings {
        assert_eq!(back.buildings[id].parking_used, bld.parking_used, "spaces in use match the cars in them");
    }
    assert_consistent(&back);
    assert_consistent(&a);
}

#[test]
fn trucks_go_back_to_the_business_that_sent_them() {
    let (mut w, shop, _lot) = town();
    // A van from the shop on a job.
    let (fs, fp) = w.truck_stop(shop).unwrap();
    let far = w.lots.values().filter(|l| l.car_segment.is_some()).max_by(|a, b| a.center.distance(fp).total_cmp(&b.center.distance(fp))).unwrap();
    let (route, s0, s1) = w.traffic.net.find_route(fs, fp, far.car_segment.unwrap(), far.car_point).expect("route");
    let id = w.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
    w.truck_dispatched(id, Some(shop));
    assert_eq!(w.traffic.cars[id].errand, Errand::Haul { base: Some(shop) });
    let parked0 = w.vehicles.fleet_parked.get(&shop).copied().unwrap_or(0);
    let mut went_back = false;
    let mut minds = IdleMind;
    for _ in 0..900 {
        w.tick(&mut minds);
        match w.traffic.cars.get(id) {
            Some(c) if c.errand == Errand::Return(shop) => went_back = true,
            Some(_) => {}
            None => break,
        }
    }
    assert!(went_back, "it turned back for base after the drop");
    assert!(!w.traffic.cars.contains_key(id), "and got there");
    assert_eq!(w.vehicles.fleet_parked.get(&shop).copied().unwrap_or(0), parked0 + 1, "parked at the shop");
}

#[test]
fn nobody_parks_on_a_roundabout_ring() {
    let mut w = World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
    w.add_roundabout(Vec2::ZERO, 30.0, 1).expect("ring");
    w.implement();
    let ring: Vec<SegmentId> = w.roads.alive_segments().map(|(id, _)| id).filter(|&id| w.on_ring(id)).collect();
    assert!(ring.len() >= 4, "the ring's segments are recognised ({})", ring.len());
    assert!(ring.iter().all(|&id| !w.kerb_allowed(id) && w.kerb_slots(id) == 0), "no kerb parking on the ring");
    // Arms keep their kerbs.
    assert!(w.roads.alive_segments().any(|(id, s)| s.is_street() && !w.on_ring(id) && w.kerb_allowed(id)));
}


#[test]
fn a_driver_hunting_for_a_space_turns_into_a_lot_with_room_it_passes() {
    // [parking] F5: "looking for parking" but driving past lots with room.
    let (mut w, shop, lot) = town();
    let c = resident(&mut w);
    let home = w.citizens[c].home.unwrap();
    let vid = w.give_car_parked(c, w.building_entrance(home).unwrap());
    assert!(w.lot_open(lot), "the lot has room");
    let (seg, pt) = w.truck_stop(lot).unwrap();
    let (lane, s_gate, _) = w.traffic.net.nearest_lane_on(seg, pt).unwrap();
    let len = w.traffic.net.lanes[lane as usize].length;
    let s0 = (s_gate - 50.0).max(0.0);
    assert!(s_gate - s0 > 5.0 && len - s_gate > 2.0, "room before and after the gate");
    let spot = *w.vehicles.cars[vid].spot().unwrap();
    // Circling with a free kerb slot as its goal, past the lot's gate.
    let car = w.launch(vid, spot, vec![lane], s0, len - 0.5, c, Errand::Search { tries: 1 }).expect("on the road");
    if w.traffic.cars[car].entering {
        w.traffic.cars[car].entering = false;
        w.traffic.insert_sorted(lane, car);
    }
    w.citizens[c].activity = Activity::Driving { car, to: Some(shop), purpose: Purpose::Shop(Resource::Goods) };
    w.citizens[c].inside = None;
    w.hunters_spot_lots();
    assert_eq!(w.traffic.cars[car].errand, Errand::ToLot(lot), "turns in at the lot it is passing");
    let used0 = w.buildings[lot].parking_used;
    until_parked(&mut w, vid, 300);
    let spot = *w.vehicles.cars[vid].spot().expect("parked");
    assert_eq!((spot.kind, spot.building), (ParkKind::Lot, Some(lot)), "in the lot");
    assert_eq!(w.buildings[lot].parking_used, used0 + 1);
    assert_consistent(&w);
}

#[test]
fn a_surface_lot_has_the_spaces_it_is_drawn_with() {
    // [parking] F35: "13 of 30 spaces but I only see 3 cars". The lot's
    // spaces are the bays its parcel fits, the bays it is drawn with.
    let (w, _shop, lot) = town();
    let b = &w.buildings[lot];
    let bays = crate::parking_layout::surface_bays(&w.lots[b.lot]);
    assert_eq!(b.parking_capacity as usize, bays.len());
    for (p, _) in &bays {
        assert!(crate::geom::point_in_polygon(*p, &w.lots[b.lot].polygon));
    }
}
