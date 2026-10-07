//! [parking] Tests of parking enforcement, towing, the pound and the lots
//! the city runs.

use super::*;
use crate::enforcement::*;
use crate::mind::IdleMind;
use crate::services::ServiceKind;
use crate::vehicle::{CarEvent, CarPlace, ParkKind, VehicleId};
use super::cars::HUNT_GIVE_UP;

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

/// Shops west of a cross street, homes east, a long empty street south.
fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    run(&mut w, 12 * 60);
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
    run(&mut w, 60);
    w.economy.treasury = 500_000.0;
    w
}

/// A grown-up resident with a car parked on the empty south street, far
/// from home (no permit there).
fn parked_far(w: &mut World) -> (CitizenId, VehicleId) {
    // Someone without a car, else one whose car stands somewhere else
    // (not one this test already parked on the south street).
    let ours = |w: &World, c: &Citizen| c.car.and_then(|v| w.vehicles.cars.get(v)).is_some_and(|car| car.spot().is_none_or(|s| s.pos.y < -200.0));
    let id = w.citizens.iter().filter(|(_, c)| c.home.is_some() && c.can_work() && !ours(w, c)).min_by_key(|(_, c)| c.car.is_some()).map(|(id, _)| id).expect("someone moved in");
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
    let taken = w.vehicles.cars.len();
    let vid = w.give_car_parked(id, Vec2::new(0.0, -240.0 - taken as f32 * 7.0));
    let spot = *w.vehicles.cars[vid].spot().unwrap();
    assert_eq!(spot.kind, ParkKind::Kerb, "a legal kerb slot on the south street");
    assert!(w.building_entrance(home).unwrap().distance(spot.pos) > PERMIT_RADIUS);
    (id, vid)
}

fn set_since(w: &mut World, vid: VehicleId, since: u64) {
    if let CarPlace::Parked(s) = &mut w.vehicles.cars[vid].place {
        s.since = since;
    }
}

fn purse(w: &World, id: CitizenId) -> f32 {
    let c = &w.citizens[id];
    c.household.and_then(|h| w.households.get(h)).map_or(c.savings, |h| h.money)
}

/// A tow company on a free lot with four staff.
fn tow_yard(w: &mut World) -> BuildingId {
    let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.car_access() && l.building.is_none()).map(|(id, _)| id).next().or_else(|| w.lots.iter().filter(|(_, l)| l.usable() && l.car_access() && l.land_use == LandUse::Commercial).map(|(id, _)| id).next()).expect("a lot");
    w.take_over_lot(lot, "a tow yard").unwrap();
    let bz = Business::new(BusinessKind::TowYard, 4);
    let id = w.place_special(lot, "Hook & Haul".into(), 4, 0, |b| b.business = Some(bz));
    let staff: Vec<CitizenId> = w.citizens.keys().take(4).collect();
    w.buildings[id].workers = staff;
    id
}

fn assert_no_car_lost(w: &World) {
    let v = &w.vehicles;
    assert_eq!(v.cars.len() as i64, v.stats.created_total as i64 - v.stats.destroyed_total as i64, "{:?}", v.stats.events_total);
    for k in v.stats.events_total.keys() {
        assert!(["arrived_with_household", "bought", "bought_out_of_town", "sold", "scrapped", "emigrated"].contains(&k.as_str()), "{k}");
    }
}

#[test]
fn two_hours_at_the_kerb_earn_a_ticket() {
    let mut w = town();
    let (id, vid) = parked_far(&mut w);
    let (_, other) = parked_far(&mut w);
    let now = w.clock.tick;
    set_since(&mut w, vid, now - KERB_LIMIT - 1);
    set_since(&mut w, other, now - 60);
    let money = purse(&w, id);
    w.check_parked(vid);
    w.check_parked(other);
    assert!(w.vehicles.cars[vid].spot().unwrap().ticketed > 0, "ticketed");
    assert_eq!(w.vehicles.cars[other].spot().unwrap().ticketed, 0, "an hour is fine");
    assert!((money - purse(&w, id) - FINE).abs() < 0.01, "the fine came out of the household");
    assert!(w.economy.ledger.today_of(Flow::ParkingFines) >= FINE, "into the treasury as parking fines");
    assert!(w.citizens[id].memories.iter().any(|m| m.text.contains("Parking ticket")));
    assert!(w.enforcement.jobs.is_empty(), "no tow yet: there is a grace");
    // A resident by their own home has a permit.
    let home = w.building_entrance(w.citizens[id].home.unwrap()).unwrap();
    let near: Vec<VehicleId> = w.vehicles.cars.iter().filter(|(_, c)| c.spot().is_some_and(|s| s.kind == ParkKind::Kerb)).map(|(v, _)| v).collect();
    for v in near {
        let owner = w.vehicles.cars[v].owner;
        let h = w.citizens.get(owner).and_then(|c| c.home).and_then(|h| w.building_entrance(h));
        if h.is_some_and(|h| h.distance(w.vehicles.cars[v].spot().unwrap().pos) < PERMIT_RADIUS) {
            assert_eq!(w.kerb_limit(v), None, "permit holders have no limit");
        }
    }
    let _ = home;
}

#[test]
fn after_the_grace_a_company_in_town_tows_it_and_the_owner_pays_to_fetch_it() {
    let mut w = town();
    let yard = tow_yard(&mut w);
    let (id, vid) = parked_far(&mut w);
    let now = w.clock.tick;
    set_since(&mut w, vid, now - KERB_LIMIT - GRACE - 10);
    w.check_parked(vid);
    assert!(w.enforcement.jobs.is_empty());
    if let CarPlace::Parked(s) = &mut w.vehicles.cars[vid].place {
        s.ticketed = now - GRACE - 1;
    }
    w.check_parked(vid);
    let job = *w.enforcement.job_for(vid).expect("tow called");
    assert_eq!(job.company, Some(yard));
    assert_eq!(job.reason, TowReason::Overstay);
    let mut hooked = false;
    for _ in 0..600 {
        run(&mut w, 1);
        let k = w.vehicles.cars[vid].spot().map(|s| s.kind);
        if k == Some(ParkKind::Hooked) {
            hooked = true;
            assert!(!w.driving.tows.is_empty(), "drawn on the tow truck");
        }
        if k == Some(ParkKind::Pound) {
            break;
        }
    }
    assert!(hooked, "hooked on a truck");
    let spot = *w.vehicles.cars[vid].spot().unwrap();
    assert_eq!(spot.kind, ParkKind::Pound);
    assert_eq!(spot.building, Some(yard), "in the company's pound");
    assert_eq!(w.enforcement.stats.tows_total, 1);
    assert_no_car_lost(&w);
    // The owner fetches it: travels to the yard, pays, drives home.
    let money_yard = w.buildings[yard].business.as_ref().unwrap().money;
    if let Some(h) = w.citizens[id].household {
        w.households[h].money = 1000.0;
    }
    while !(9..17).contains(&w.clock.hour()) {
        run(&mut w, 1);
    }
    for _ in 0..3000 {
        run(&mut w, 1);
        if w.enforcement.stats.released_total > 0 {
            break;
        }
    }
    assert_eq!(w.enforcement.stats.released_total, 1, "released");
    assert!(w.buildings[yard].business.as_ref().unwrap().money >= money_yard + TOW_FEE, "the yard was paid");
    assert!(!w.vehicles.cars[vid].spot().is_some_and(|s| s.kind == ParkKind::Pound), "out of the pound");
    assert_no_car_lost(&w);
}

#[test]
fn illegal_parking_is_towed_at_once_from_out_of_town_when_no_company_exists() {
    let mut w = town();
    let (id, vid) = parked_far(&mut w);
    // Move it onto a street where parking is banned (the shop fronts).
    let shop_front = w.lots.values().find(|l| l.land_use == LandUse::Commercial).map(|l| l.entrance()).unwrap();
    w.refresh_no_kerb();
    let k = w.free_illegal_slots_near(shop_front, 200.0, 1).into_iter().next().expect("a banned kerb");
    w.unpark(vid);
    let spot = w.kerb_spot(k);
    assert_eq!(spot.kind, ParkKind::Overflow);
    w.park_at(vid, spot);
    w.check_parked(vid);
    let job = *w.enforcement.job_for(vid).expect("towed at once");
    assert_eq!(job.reason, TowReason::Illegal);
    assert_eq!(job.company, None, "no tow company in town: one from out of town");
    for _ in 0..1200 {
        run(&mut w, 1);
        if w.vehicles.cars[vid].spot().is_some_and(|s| s.kind == ParkKind::Pound) {
            break;
        }
    }
    let spot = *w.vehicles.cars[vid].spot().expect("impounded");
    assert_eq!(spot.kind, ParkKind::Pound);
    assert_eq!(spot.building, None, "in a pound out of town");
    assert!(w.clock.tick >= job.since + OUT_OF_TOWN_DELAY, "slower");
    assert_eq!(w.enforcement.stats.tows_out_of_town_total, 1);
    assert!(w.release_fee(&spot) >= OUT_OF_TOWN_TOW_FEE, "and dearer");
    // Paid for and delivered home from out of town.
    if let Some(h) = w.citizens[id].household {
        w.households[h].money = 1000.0;
    }
    for _ in 0..3000 {
        run(&mut w, 1);
        if w.vehicles.cars[vid].spot().is_some_and(|s| s.kind != ParkKind::Pound) {
            break;
        }
    }
    assert!(w.enforcement.stats.released_total >= 1);
    assert!(w.vehicles.cars[vid].spot().is_some_and(|s| s.kind != ParkKind::Pound), "delivered back and parked");
    assert_no_car_lost(&w);
}

#[test]
fn unclaimed_cars_are_auctioned_never_deleted() {
    let mut w = town();
    let yard = tow_yard(&mut w);
    let (id, vid) = parked_far(&mut w);
    w.call_tow(vid, TowReason::Overstay);
    for _ in 0..600 {
        run(&mut w, 1);
        if w.vehicles.cars[vid].spot().is_some_and(|s| s.kind == ParkKind::Pound) {
            break;
        }
    }
    assert!(w.vehicles.cars[vid].spot().is_some_and(|s| s.kind == ParkKind::Pound && s.building == Some(yard)));
    // Broke: nobody comes for it.
    if let Some(h) = w.citizens[id].household {
        w.households[h].money = -500.0;
    }
    let now = w.clock.tick;
    set_since(&mut w, vid, now - AUCTION_AFTER - 1);
    run(&mut w, 70);
    assert_eq!(w.enforcement.stats.auctioned_total, 1);
    assert!(w.citizens[id].car.is_none(), "no longer theirs");
    assert!(w.citizens[id].memories.iter().any(|m| m.text.contains("auctioned")));
    // It drives off to its buyer (or is recorded sold): never vanishes.
    for _ in 0..900 {
        if !w.vehicles.cars.contains_key(vid) {
            break;
        }
        run(&mut w, 1);
    }
    assert!(!w.vehicles.cars.contains_key(vid));
    assert!(w.vehicles.stats.events_total.get("sold").copied().unwrap_or(0) >= 1, "sold at auction");
    assert_no_car_lost(&w);
}

#[test]
fn officers_patrol_and_ticket_overstayers() {
    let mut w = town();
    let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.car_access() && l.land_use == LandUse::Commercial).map(|(id, _)| id).next().unwrap();
    let office = w.build_service(lot, ServiceKind::ParkingEnforcement, Tier::Mid).expect("office");
    let staff: Vec<CitizenId> = w.citizens.keys().take(6).collect();
    w.buildings[office].workers = staff;
    let mut cars = Vec::new();
    for _ in 0..4 {
        let (_, vid) = parked_far(&mut w);
        cars.push(vid);
    }
    // To the morning shift.
    while !(9..17).contains(&w.clock.hour()) {
        run(&mut w, 1);
    }
    let now = w.clock.tick;
    for &v in &cars {
        set_since(&mut w, v, now - KERB_LIMIT - 5);
    }
    let mut saw_patrol = false;
    for _ in 0..600 {
        run(&mut w, 1);
        saw_patrol |= !w.enforcement.patrols.is_empty();
        if w.enforcement.stats.tickets_total > 0 {
            break;
        }
    }
    assert!(saw_patrol, "officers went out");
    assert!(w.enforcement.stats.tickets_total > 0, "and wrote tickets");
}

#[test]
fn the_city_places_parking_lots_of_three_sizes() {
    let mut w = town();
    let mut lots: Vec<(f32, LotId)> = w.lots.iter().filter(|(_, l)| l.usable() && l.car_access() && l.land_use != LandUse::Park).map(|(id, l)| (l.area(), id)).collect();
    lots.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut placed = Vec::new();
    for size in LotSize::ALL {
        let lot = lots.iter().find(|(a, l)| *a >= size.min_area() && !placed.iter().any(|(p, _)| p == l)).map(|(_, l)| *l).expect("a lot big enough");
        let treasury = w.economy.treasury;
        let b = w.place_on_lot(crate::services::Placeable::ParkingLot(size), lot).expect("placed");
        assert!(w.economy.treasury < treasury, "paid for");
        assert_eq!(w.buildings[b].parking_capacity, size.capacity());
        assert_eq!(w.buildings[b].floors, size.floors());
        assert!(w.buildings[b].is_parking());
        assert_eq!(w.vehicles.city_lots[&b].price, size.default_price());
        placed.push((lot, b));
    }
    assert!(LotSize::Small.capacity() < LotSize::Large.capacity() && LotSize::Large.capacity() < LotSize::MultiStorey.capacity());
    let (_, b) = placed[0];
    assert_eq!(w.set_parking_price(b, 4.5), Ok(4.5));
    // Drivers' search uses them; the fee goes to the treasury.
    let (id, vid) = parked_far(&mut w);
    let _ = id;
    w.unpark(vid);
    let spot = w.building_spot(ParkKind::Lot, b).unwrap();
    w.park_at(vid, spot);
    assert_eq!(w.buildings[b].parking_used, 1);
    let fees = w.economy.ledger.today_of(Flow::ParkingFees);
    w.pay_lot(id, b);
    assert!(w.economy.ledger.today_of(Flow::ParkingFees) > fees, "fees to the treasury");
    assert!(w.lot_open(b));
    assert_no_car_lost(&w);
}

#[test]
fn enforcement_runs_are_deterministic() {
    let make = || {
        let mut w = town();
        tow_yard(&mut w);
        let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.car_access() && l.land_use == LandUse::Commercial && l.building.is_some()).map(|(id, _)| id).nth(1).unwrap();
        let office = w.build_service(lot, ServiceKind::ParkingEnforcement, Tier::High).unwrap();
        let staff: Vec<CitizenId> = w.citizens.keys().take(8).collect();
        w.buildings[office].workers = staff;
        for _ in 0..3 {
            let (_, v) = parked_far(&mut w);
            let now = w.clock.tick;
            set_since(&mut w, v, now.saturating_sub(KERB_LIMIT + GRACE + 30));
        }
        run(&mut w, 1440);
        w
    };
    let (a, b) = (make(), make());
    let places = |w: &World| w.vehicles.cars.iter().map(|(id, c)| (id, format!("{:?}", c.place))).collect::<Vec<_>>();
    assert_eq!(places(&a), places(&b));
    assert_eq!(format!("{:?}", a.enforcement.stats), format!("{:?}", b.enforcement.stats));
    let _ = CarEvent::Sold;
}

/// [parking] The company from out of town does not send a truck per call:
/// with its few trucks out, the other jobs wait their turn. (A city short
/// of parking had 170 tow trucks on its roads.)
#[test]
fn the_out_of_town_tow_company_sends_only_its_few_trucks() {
    let mut w = town();
    let shop_front = w.lots.values().find(|l| l.land_use == LandUse::Commercial).map(|l| l.entrance()).unwrap();
    w.refresh_no_kerb();
    let n = OUT_OF_TOWN_TOW_TRUCKS + 2;
    let mut cars = Vec::new();
    let all: Vec<VehicleId> = (0..n).map(|_| parked_far(&mut w).1).collect();
    for vid in all {
        let k = w.free_illegal_slots_near(shop_front, 300.0, 1).into_iter().next().expect("a banned kerb");
        w.unpark(vid);
        let spot = w.kerb_spot(k);
        w.park_at(vid, spot);
        w.check_parked(vid);
        assert!(w.enforcement.job_for(vid).is_some_and(|j| j.company.is_none()), "called from out of town");
        cars.push(vid);
    }
    let mut most = 0;
    for _ in 0..4000 {
        run(&mut w, 1);
        let out = w.enforcement.jobs.iter().filter(|j| j.truck.is_some_and(|t| w.traffic.cars.contains_key(t))).count();
        most = most.max(out);
        if cars.iter().all(|&v| w.vehicles.cars[v].spot().is_some_and(|s| s.kind == ParkKind::Pound)) {
            break;
        }
    }
    assert!(most <= OUT_OF_TOWN_TOW_TRUCKS, "{most} trucks out at once");
    assert!(most > 0);
    assert!(cars.iter().all(|&v| w.vehicles.cars[v].spot().is_some_and(|s| s.kind == ParkKind::Pound)), "every car towed in the end");
    assert_no_car_lost(&w);
}

/// [parking] A driver who has hunted for a space for over an hour stops
/// looking and leaves the car where it can, instead of being sent round
/// the block again; one only just started goes on looking.
#[test]
fn an_hour_of_hunting_for_a_space_ends_with_the_car_left_somewhere() {
    let mut w = town();
    let (id, vid) = parked_far(&mut w);
    let (id2, vid2) = parked_far(&mut w);
    let (seg, pt) = w.roads.nearest_street(Vec2::new(-150.0, 0.0)).map(|(s, _, q)| (s, q)).unwrap();
    let tick = w.clock.tick;
    let drive = |w: &mut World, id: CitizenId, vid: VehicleId, since: u64| {
        let car = w.drive_from_spot(vid, seg, pt, id, crate::vehicle::Errand::Search { tries: 1 }).expect("on the road");
        w.citizens[id].commute.search_since = since;
        // Out of its own space and along the shop street, where parking
        // is banned, as if it had got there.
        let (goal, s_end, _) = w.traffic.net.nearest_lane_on(seg, pt).unwrap();
        w.traffic.cars[car].entering = false;
        w.traffic.reroute(car, vec![goal], s_end, s_end);
        w.traffic.cars[car].pos = pt;
        car
    };
    let tired = drive(&mut w, id, vid, tick.saturating_sub(HUNT_GIVE_UP + 5).max(1));
    let fresh = drive(&mut w, id2, vid2, tick);
    assert!(w.vehicle_arrived(tired));
    assert!(w.vehicle_arrived(fresh));
    assert!(!w.traffic.cars.contains_key(tired), "the tired hunter left the car");
    assert!(w.vehicles.cars[vid].spot().is_some(), "parked");
    assert!(w.traffic.cars.contains_key(fresh) && w.vehicles.cars[vid2].spot().is_none(), "the fresh one drives on to look: {:?} {:?}", w.traffic.cars.get(fresh).map(|c| c.errand), w.vehicles.cars[vid2].spot().map(|s| (s.kind, s.pos)));
    assert_no_car_lost(&w);
}
