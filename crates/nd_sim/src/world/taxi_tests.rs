//! [taxis] Tests of taxi companies, cabs and their riders.

use super::trips::{ModePlan, TripQuery};
use super::*;
use crate::citizens::TripMode;
use crate::mind::IdleMind;
use crate::taxi::{self, CabJob, CabPlace};

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

fn finish_all(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

/// A long main street: shops at the west end, homes at the east end, a
/// vacant commercial block in the middle for a taxi yard.
fn town() -> (World, BuildingId) {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], 1, 1);
    w.add_zone(square(-650.0, 10.0, 180.0), LandUse::Commercial);
    w.add_zone(square(-120.0, -190.0, 110.0), LandUse::Commercial);
    w.add_zone(square(470.0, 10.0, 180.0), LandUse::Residential);
    let shops: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Commercial && l.car_access() && l.center.x < -400.0).map(|(id, _)| id).collect();
    assert!(!shops.is_empty());
    w.build_on(shops[0]);
    let shop = w.lots[shops[0]].building.unwrap();
    w.finish_construction(shop);
    run(&mut w, 10 * 60);
    finish_all(&mut w);
    run(&mut w, 60);
    (w, shop)
}

/// A vacant lot for the taxi yard, near the middle.
fn yard_lot(w: &World) -> LotId {
    w.lots.iter().filter(|(_, l)| l.building.is_none() && l.usable() && l.land_use == LandUse::Commercial && l.car_segment.is_some() && l.area() >= taxi::YARD_MIN_AREA).min_by(|a, b| a.1.center.length().total_cmp(&b.1.center.length())).map(|(id, _)| id).expect("a vacant commercial lot")
}

/// A grown-up at home with money and no car or bike.
fn resident(w: &mut World) -> CitizenId {
    let id = w.citizens.iter().find(|(_, c)| c.home.is_some() && c.can_work()).map(|(id, _)| id).expect("someone moved in");
    if let Some(vid) = w.citizens[id].car {
        w.drop_car(vid, crate::vehicle::CarEvent::Sold);
    }
    let home = w.citizens[id].home.unwrap();
    let c = &mut w.citizens[id];
    c.owns_bike = false;
    c.activity = Activity::AtHome;
    c.inside = Some(home);
    c.savings = 500.0;
    c.income = 120.0;
    c.next_decision_tick = u64::MAX;
    id
}

/// Every cab is somewhere: in its yard, at a rank or kerb, or on the road
/// as a traffic car whose errand is that cab.
fn assert_cabs_have_a_place(w: &World) {
    for (id, c) in &w.taxis.cabs {
        if let CabPlace::Road(car) = c.place {
            let t = w.traffic.cars.get(car).unwrap_or_else(|| panic!("cab {id:?} on a vanished car"));
            assert_eq!(t.errand, crate::vehicle::Errand::Taxi(id));
        }
    }
}

#[test]
fn a_rider_hails_a_cab_rides_it_and_pays_the_company() {
    let (mut w, shop) = town();
    let lot = yard_lot(&w);
    let company = w.build_taxi_company(lot, "Test Cabs".into()).expect("company opens");
    finish_all(&mut w);
    assert_eq!(w.taxis.fleet_size(company), taxi::START_CABS);
    run(&mut w, 15);
    assert!(w.taxis_in_service(), "some cabs on duty");
    let c = resident(&mut w);
    // Late and drunk: a cab beats walking 1 km home.
    w.citizens[c].drunk = 0.8;
    let from = w.building_entrance(w.citizens[c].home.unwrap()).unwrap();
    let to = w.building_entrance(shop).unwrap();
    let q = TripQuery { from, to, target: shop, purpose: Purpose::Visit(LandUse::Commercial), allow_transit: true, allow_car: true, allow_bike: true };
    let opts = w.mode_options(c, &q);
    let cab = opts.iter().find(|o| o.mode == TripMode::Taxi).expect("a taxi option");
    let walk = opts.iter().find(|o| o.mode == TripMode::Walk).unwrap();
    assert!(cab.cost < walk.cost, "a drunk takes the cab over a long walk: {} vs {}", cab.cost, walk.cost);
    let ModePlan::Taxi { segment, at, wait_at } = cab.plan else { panic!() };
    let money0 = w.buildings[company].business.as_ref().unwrap().money;
    let hh0 = w.citizens[c].household.and_then(|h| w.households.get(h)).map_or(w.citizens[c].savings, |h| h.money);
    w.hail_taxi(c, segment, at, wait_at, shop, Purpose::Visit(LandUse::Commercial), true);
    w.begin_trip(c, TripMode::Taxi, from.distance(to));
    let mut rode = false;
    let mut minds = IdleMind;
    for _ in 0..400 {
        w.tick(&mut minds);
        assert_cabs_have_a_place(&w);
        if matches!(w.citizens[c].activity, Activity::Riding { .. }) {
            rode = true;
        }
        if rode && !matches!(w.citizens[c].activity, Activity::Riding { .. } | Activity::Waiting { .. }) {
            break;
        }
    }
    assert!(rode, "the rider got into a cab: {:?}", w.citizens[c].activity);
    assert_eq!(w.taxis.stats.rides_today, 1);
    assert_eq!(w.taxis.stats.drunk_rides_today, 1);
    let money1 = w.buildings[company].business.as_ref().unwrap().money;
    assert!(money1 > money0, "the company took the fare");
    let hh1 = w.citizens[c].household.and_then(|h| w.households.get(h)).map_or(w.citizens[c].savings, |h| h.money);
    assert!(hh1 < hh0, "the rider paid");
    // Set down near the shop, walking in.
    assert!(w.citizens[c].pos.distance(to) < 120.0, "set down by the shop: {:?}", w.citizens[c].pos);
    assert_eq!(w.taxis.fleet_size(company), taxi::START_CABS, "no cab vanished");
    run(&mut w, 200);
    assert_cabs_have_a_place(&w);
    assert!(w.taxis.cabs.values().all(|x| x.job != CabJob::Fare { rider: c, from: Vec2::ZERO }));
}

#[test]
fn unserved_demand_opens_a_company_near_it() {
    let (mut w, _) = town();
    assert!(w.taxi_companies().is_empty());
    let vacant: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.building.is_none() && l.usable()).map(|(id, _)| id).collect();
    assert!(!w.maybe_found_taxi_company(&vacant), "no demand, no company");
    w.taxis.latent_today = taxi::FOUND_DEMAND + 2;
    w.taxis.latent_sum = Vec2::new(-60.0, -100.0) * w.taxis.latent_today as f32;
    assert!(w.maybe_found_taxi_company(&vacant), "demand opens one");
    let companies = w.taxi_companies();
    assert_eq!(companies.len(), 1);
    let b = &w.buildings[companies[0]];
    assert_eq!(b.land_use, LandUse::Commercial);
    assert!(b.name.ends_with("Cabs") || b.name.contains("Tax") || b.name.contains("Cab"), "{}", b.name);
    assert_eq!(w.taxis.fleet_size(companies[0]), taxi::START_CABS);
    assert!(w.taxis.cabs.values().all(|c| c.place == CabPlace::Yard), "new cabs stand in the yard");
    assert_eq!(w.taxis.latent_today, 0);
}

#[test]
fn cabs_sleep_in_the_yard_at_dawn_and_work_the_night() {
    let (mut w, _) = town();
    let lot = yard_lot(&w);
    let company = w.build_taxi_company(lot, "Test Cabs".into()).unwrap();
    // Grow the fleet so the shares tell.
    for _ in 0..7 {
        w.add_cab_for_test(company);
    }
    let n = w.taxis.fleet_size(company);
    // Run to 23:00: all on duty, some out at ranks.
    while w.clock.hour() != 23 {
        run(&mut w, 1);
    }
    run(&mut w, 40);
    let on = w.taxis.cabs.values().filter(|c| c.on_duty).count();
    assert_eq!(on, n, "the whole fleet works the night");
    // On to 05:00: most back in the yard.
    while !(w.clock.hour() == 4 && w.clock.minute_of_day() % 60 == 0) {
        run(&mut w, 1);
        assert_cabs_have_a_place(&w);
    }
    run(&mut w, 59);
    let on = w.taxis.cabs.values().filter(|c| c.on_duty).count();
    let in_yard = w.taxis.cabs.values().filter(|c| c.place == CabPlace::Yard).count();
    assert!(on <= (n as f32 * 0.3).ceil() as usize + 1, "few on duty at dawn: {on} of {n}");
    // Midnight's books may have sold a cab that stood idle all day.
    let fleet = w.taxis.fleet_size(company);
    assert_eq!(fleet + w.taxis.stats.cabs_sold_total as usize, n, "every cab is in the fleet or was sold");
    assert!(in_yard >= fleet - on - 1, "the rest park in the yard: {in_yard} of {fleet}");
}

#[test]
fn a_closed_company_sells_its_cabs_out_of_town() {
    let (mut w, _) = town();
    let lot = yard_lot(&w);
    let company = w.build_taxi_company(lot, "Test Cabs".into()).unwrap();
    run(&mut w, 30);
    w.buildings[company].business.as_mut().unwrap().closed = true;
    let mut minds = IdleMind;
    for _ in 0..600 {
        w.tick(&mut minds);
        assert_cabs_have_a_place(&w);
        if w.taxis.cabs.is_empty() {
            break;
        }
    }
    assert!(w.taxis.cabs.is_empty(), "every cab drove off: {:?}", w.taxis.cabs.values().map(|c| (c.place, c.job)).collect::<Vec<_>>());
    assert_eq!(w.taxis.stats.cabs_sold_total as usize, taxi::START_CABS);
}

#[test]
fn a_rider_with_no_cab_coming_gives_up_and_goes_another_way() {
    let (mut w, shop) = town();
    let lot = yard_lot(&w);
    let company = w.build_taxi_company(lot, "Test Cabs".into()).unwrap();
    run(&mut w, 10);
    let c = resident(&mut w);
    let from = w.building_entrance(w.citizens[c].home.unwrap()).unwrap();
    let to = w.building_entrance(shop).unwrap();
    let q = TripQuery { from, to, target: shop, purpose: Purpose::Visit(LandUse::Commercial), allow_transit: true, allow_car: true, allow_bike: true };
    let opt = w.mode_options(c, &q).into_iter().find(|o| o.mode == TripMode::Taxi).expect("taxi option");
    let ModePlan::Taxi { segment, at, wait_at } = opt.plan else { panic!() };
    // Every cab off the rota and kept there.
    let ids: Vec<_> = w.taxis.fleet(company).map(|(id, _)| id).collect();
    w.hail_taxi(c, segment, at, wait_at, shop, Purpose::Visit(LandUse::Commercial), true);
    let mut minds = IdleMind;
    for _ in 0..300 {
        for id in &ids {
            w.taxis.cabs[*id].on_duty = false;
        }
        w.tick(&mut minds);
        if w.taxis.stats.gave_up_today > 0 {
            break;
        }
    }
    assert_eq!(w.taxis.stats.gave_up_today, 1);
    assert!(w.citizens[c].transit.is_none(), "not waiting any more");
    assert!(matches!(w.citizens[c].activity, Activity::Traveling { .. }), "on the way another way: {:?}", w.citizens[c].activity);
    assert!(w.taxis.missed.get(&company).is_some_and(|m| m.0 >= 1), "counted against the company");
}

#[test]
fn no_taxi_option_without_a_company() {
    let (mut w, shop) = town();
    let c = resident(&mut w);
    let from = w.building_entrance(w.citizens[c].home.unwrap()).unwrap();
    let to = w.building_entrance(shop).unwrap();
    let q = TripQuery { from, to, target: shop, purpose: Purpose::Visit(LandUse::Commercial), allow_transit: true, allow_car: true, allow_bike: true };
    assert!(!w.mode_options(c, &q).iter().any(|o| o.mode == TripMode::Taxi));
    // ... but the trip is counted as demand when a cab would have won.
    w.citizens[c].drunk = 0.9;
    let ranked = w.choose_mode(c, &q);
    w.note_taxi_demand(c, &q, &ranked);
    assert_eq!(w.taxis.latent_today, 1);
}

impl World {
    fn add_cab_for_test(&mut self, company: BuildingId) {
        let number = self.taxis.fleet(company).map(|(_, c)| c.number).max().unwrap_or(0) + 1;
        let pos = self.building_lot(company).map_or(Vec2::ZERO, |l| l.center);
        self.taxis.cabs.insert(crate::taxi::Cab { company, number, model: taxi::cab_model(number), place: CabPlace::Yard, job: CabJob::Idle, on_duty: false, last_pos: pos, since: 0 });
    }
}

/// [review] A cab sent to a full rank waited at the kerb still `ToRank`:
/// counted at the rank and never moved on by the rota again.
#[test]
fn a_cab_at_a_full_rank_is_free_for_the_rota() {
    let (mut w, shop) = town();
    let yard = w.build_taxi_company(yard_lot(&w), "Test Cabs".into()).expect("a yard");
    finish_all(&mut w);
    run(&mut w, 15);
    // A rider hails one: a cab comes out on the road.
    let c = resident(&mut w);
    w.citizens[c].drunk = 0.8;
    let from = w.building_entrance(w.citizens[c].home.unwrap()).unwrap();
    let to = w.building_entrance(shop).unwrap();
    let q = TripQuery { from, to, target: shop, purpose: Purpose::Visit(LandUse::Commercial), allow_transit: true, allow_car: true, allow_bike: true };
    let opts = w.mode_options(c, &q);
    let ModePlan::Taxi { segment, at, wait_at } = opts.iter().find(|o| o.mode == TripMode::Taxi).expect("a taxi option").plan else { panic!() };
    w.hail_taxi(c, segment, at, wait_at, shop, Purpose::Visit(LandUse::Commercial), true);
    let mut on_road = None;
    for _ in 0..400 {
        run(&mut w, 1);
        on_road = w.taxis.cabs.iter().find_map(|(id, c)| if let CabPlace::Road(car) = c.place { Some((id, car)) } else { None });
        if on_road.is_some() {
            break;
        }
    }
    let (id, car) = on_road.expect("a cab on the road");
    // The shop's rank, every slot taken.
    let model = w.taxis.cabs[id].model;
    let (segment, _) = w.roads.alive_segments().next().unwrap();
    for slot in 0..taxi::RANK_SLOTS as u8 {
        w.taxis.cabs.insert(crate::taxi::Cab { company: yard, number: 90 + slot as u16, model, place: CabPlace::Rank { rank: shop, slot, segment, pos: Vec2::ZERO, heading: 0.0 }, job: CabJob::Idle, on_duty: true, last_pos: Vec2::ZERO, since: 0 });
    }
    w.taxis.cabs[id].job = CabJob::ToRank { rank: shop };
    w.cab_arrived(car, id);
    assert!(!matches!(w.taxis.cabs[id].place, CabPlace::Rank { .. } | CabPlace::Road(_)), "waits at the kerb");
    assert_eq!(w.taxis.cabs[id].job, CabJob::Idle, "and the rota may send it on");
}
