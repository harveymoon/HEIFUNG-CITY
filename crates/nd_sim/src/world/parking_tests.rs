//! [parking] Tests of parking enforcement, towing, the pound and the lots
//! the city runs.

use super::*;
use crate::enforcement::*;
use crate::mind::IdleMind;
use crate::services::ServiceKind;
use crate::vehicle::{CarEvent, CarPlace, ParkKind, VehicleId};
use super::cars::HUNT_GIVE_UP;
use crate::Errand;

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
    // [under-construction] It opens once what stood there is cleared.
    w.finish_demolition(id);
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

/// [clamp] Over the limit after the grace: clamped, with a fine and a
/// time limit. Unpaid (the household is broke) the car is towed when the
/// limit runs out, the fine added to the release fee.
#[test]
fn after_the_grace_it_is_clamped_and_towed_when_the_fine_goes_unpaid() {
    let mut w = town();
    let yard = tow_yard(&mut w);
    let (id, vid) = parked_far(&mut w);
    if let Some(h) = w.citizens[id].household {
        w.households[h].money = 0.0;
    }
    w.citizens[id].savings = 0.0;
    let now = w.clock.tick;
    set_since(&mut w, vid, now - KERB_LIMIT - GRACE - 10);
    w.check_parked(vid);
    assert!(w.enforcement.jobs.is_empty());
    if let CarPlace::Parked(s) = &mut w.vehicles.cars[vid].place {
        s.ticketed = now - GRACE - 1;
    }
    w.check_parked(vid);
    let job = *w.enforcement.job_for(vid).expect("tow booked");
    assert_eq!(job.company, Some(yard));
    assert_eq!(job.reason, TowReason::Unpaid);
    assert!(matches!(job.state, TowState::Waiting { until } if until >= now + CLAMP_LIMIT), "not before the limit: {:?}", job.state);
    let spot = *w.vehicles.cars[vid].spot().unwrap();
    assert!(spot.booted, "clamped");
    assert!(w.car_ready(id, spot.pos).is_none(), "a clamped car cannot be driven");
    assert!(w.citizens[id].memories.iter().any(|m| m.text.contains("clamped")));
    assert_eq!(w.enforcement.stats.clamps_total, 1);
    // Still clamped and at the kerb an hour before the limit.
    run(&mut w, (CLAMP_LIMIT - 60) as usize);
    let s = *w.vehicles.cars[vid].spot().unwrap();
    assert!(s.booted && s.kind.on_street(), "waits out the limit at the kerb");
    let mut hooked = false;
    for _ in 0..700 {
        run(&mut w, 1);
        let k = w.vehicles.cars[vid].spot().map(|s| s.kind);
        if k == Some(ParkKind::Hooked) {
            hooked = true;
            assert!(!w.driving.tows.is_empty(), "drawn on the tow truck");
            // [tow] F34: a wrecker with a boom and a wheel lift, not a pickup.
            let truck = w.driving.tows[0].truck;
            assert_eq!(w.traffic.cars[truck].model.body, crate::vehicle::BodyType::TowTruck);
        }
        if k == Some(ParkKind::Pound) {
            break;
        }
    }
    assert!(hooked, "hooked on a truck");
    let spot = *w.vehicles.cars[vid].spot().unwrap();
    assert_eq!(spot.kind, ParkKind::Pound);
    assert_eq!(spot.building, Some(yard), "in the company's pound");
    assert!(!spot.booted, "the clamp came off with the hook");
    assert!(spot.fee >= TOW_FEE + CLAMP_FINE, "the unpaid fine is owed at the pound: {}", spot.fee);
    assert_eq!(w.enforcement.stats.tows_total, 1);
    assert_eq!(w.enforcement.stats.clamps_towed_total, 1);
    assert!(w.citizens[id].memories.iter().any(|m| m.text.contains("fine went unpaid")));
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

/// [clamp] An owner who can pay settles the clamp fine: the clamp comes
/// off, the tow is cancelled and the car can be driven again.
#[test]
fn a_clamp_fine_paid_in_time_takes_the_clamp_off() {
    let mut w = town();
    tow_yard(&mut w);
    let (id, vid) = parked_far(&mut w);
    if let Some(h) = w.citizens[id].household {
        w.households[h].money = 1000.0;
    }
    w.citizens[id].savings = 1000.0;
    while !(8..14).contains(&w.clock.hour()) {
        run(&mut w, 1);
    }
    w.clamp(vid).expect("clamped");
    let money = purse(&w, id);
    let fines = w.economy.ledger.today_of(Flow::ParkingFines);
    for _ in 0..CLAMP_LIMIT {
        run(&mut w, 1);
        if w.enforcement.stats.clamps_paid_total > 0 {
            break;
        }
    }
    assert_eq!(w.enforcement.stats.clamps_paid_total, 1, "paid within the limit");
    let spot = *w.vehicles.cars[vid].spot().unwrap();
    assert!(!spot.booted && spot.kind.on_street(), "clamp off, still at the kerb");
    assert!(w.enforcement.job_for(vid).is_none(), "the tow is cancelled");
    assert!(w.car_ready(id, spot.pos).is_some(), "drivable again");
    assert!(money - purse(&w, id) >= CLAMP_FINE - 0.01, "the household paid");
    assert!(w.economy.ledger.today_of(Flow::ParkingFines) >= fines + CLAMP_FINE - 0.01 || w.clock.minute_of_day() < 60, "into the treasury");
    assert!(w.citizens[id].memories.iter().any(|m| m.text.contains("clamp off")));
    assert_eq!(w.enforcement.stats.tows_total, 0);
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
    // A vacant shop lot, so the office opens at once (on a lot with a shop
    // the city buys it out and waits for the clearing first). [stable-
    // parcels] Lots keep their ids through rebuilds now, so "the first
    // commercial lot" is no longer the same lot as in a fresh cut.
    let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.car_access() && l.land_use == LandUse::Commercial && l.building.is_none()).map(|(id, _)| id).next().unwrap();
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
fn the_city_places_parking_lots_sized_by_their_plots() {
    let mut w = town();
    let mut lots: Vec<(f32, LotId)> = w.lots.iter().filter(|(_, l)| l.usable() && l.car_access() && l.land_use != LandUse::Park).map(|(id, l)| (l.area(), id)).collect();
    lots.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut placed = Vec::new();
    for size in LotSize::ALL {
        // (Apart from each other: a lot next to the city's own joins it.)
        let apart = |l: LotId, w: &World, placed: &Vec<(LotId, BuildingId)>| placed.iter().all(|(p, _)| w.lots.get(*p).is_none_or(|q| q.center.distance(w.lots[l].center) > 80.0));
        let lot = lots.iter().find(|(a, l)| *a >= size.min_area() && w.lots.contains_key(*l) && apart(*l, &w, &placed)).map(|(_, l)| *l).expect("a lot big enough");
        let area = w.lots[lot].area();
        let treasury = w.economy.treasury;
        let b = w.place_on_lot(crate::services::Placeable::ParkingLot(size), lot).expect("placed");
        assert!((treasury - w.economy.treasury - size.cost_for(area)).abs() < 1.0, "paid for by the plot's area");
        // [parking] A surface lot has the bays its parcel fits (F35); a car
        // park its decks' spaces, [parking-fit] by the plot's area.
        let spaces = if size.floors() == 0 { crate::parking_layout::surface_capacity(&w.lots[lot]) } else { size.capacity_for(area) };
        assert_eq!(w.buildings[b].parking_capacity, spaces);
        assert!(spaces >= 4, "{size:?}: {spaces} spaces");
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

// ── [tow-moves] against a one-way, and turning round ───────────────────────

/// A citizen (the first adult with a home) given a car parked near `at`.
fn car_near(w: &mut World, at: Vec2) -> (CitizenId, VehicleId) {
    let id = w.citizens.iter().filter(|(_, c)| c.home.is_some() && c.can_work() && c.car.is_none()).map(|(id, _)| id).next().unwrap_or_else(|| w.citizens.iter().find(|(_, c)| c.home.is_some()).map(|(id, _)| id).unwrap());
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
    let vid = w.give_car_parked(id, at);
    (id, vid)
}

/// A car parked near the end of a one-way street whose legal way in is a
/// long loop: the truck comes in from the far end and creeps the last
/// stretch against the traffic, ambers on, hooks the car and turns round.
#[test]
fn a_tow_truck_takes_the_last_stretch_of_a_one_way_against_the_traffic() {
    let mut w = town();
    // A one-way street north to the avenue at x = 300, its far end joined
    // round by a street along y = -200 to the cross street.
    w.add_road(&[Vec2::new(300.0, -200.0), Vec2::new(300.0, 0.0)], 1, 0);
    w.add_road(&[Vec2::new(0.0, -200.0), Vec2::new(300.0, -200.0)], 1, 1);
    run(&mut w, 2);
    let (_, vid) = car_near(&mut w, Vec2::new(300.0, -45.0));
    let spot = *w.vehicles.cars[vid].spot().unwrap();
    assert!(spot.kind.on_street() && (spot.pos.x - 300.0).abs() < 8.0, "parked on the one-way: {:?}", spot.pos);
    // A tow company in town, west of the cross street: the long way round
    // to the car is down the cross street and back up the one-way; from
    // out of town the truck's way in is drawn at random and may come
    // up the one-way from the south anyway.
    tow_yard(&mut w);
    let job = w.call_tow(vid, TowReason::Illegal).expect("tow called");
    let mut against = false;
    let mut truck = None;
    for _ in 0..400 {
        run(&mut w, 1);
        if let Some(t) = w.enforcement.job(job).and_then(|j| j.truck) {
            truck = Some(t);
        }
        if let Some(t) = truck.and_then(|t| w.traffic.cars.get(t)) {
            if w.traffic.in_manoeuvre(truck.unwrap()) && (t.pos.x - 300.0).abs() < 8.0 && t.pos.y < -2.0 && t.heading.sin() < -0.8 {
                against = true;
                assert!(t.drive.amber, "hazard lights on");
            }
        }
        if w.vehicles.cars[vid].spot().is_some_and(|s| s.kind == ParkKind::Pound) {
            break;
        }
    }
    assert!(w.enforcement.stats.contraflow_total >= 1, "planned against the one-way");
    assert!(against, "drove south up the north-bound one-way, slowly, off the lanes");
    assert!(w.enforcement.stats.u_turns_total >= 1, "turned round with the car on");
    assert_eq!(w.vehicles.cars[vid].spot().map(|s| s.kind), Some(ParkKind::Pound), "and took it to the pound");
    assert!(w.driving.moves.is_empty(), "no move left hanging");
    assert_no_car_lost(&w);
}

/// A truck that hooked a car facing away from its yard turns round in the
/// street instead of driving on to the dead end and back.
#[test]
fn a_tow_truck_turns_round_in_the_street_for_the_short_way_back() {
    let mut w = town();
    tow_yard(&mut w);
    // Cars on both sides of the long south street: whichever truck stops
    // facing away from the yard (north) turns round.
    let (_, a) = car_near(&mut w, Vec2::new(4.0, -150.0));
    let (_, b) = car_near(&mut w, Vec2::new(-4.0, -170.0));
    w.call_tow(a, TowReason::Illegal);
    w.call_tow(b, TowReason::Illegal);
    let mut turning = false;
    for _ in 0..900 {
        run(&mut w, 1);
        turning |= w.driving.moves.iter().any(|m| matches!(m.then, super::tow_moves::AfterMove::ToPound { .. }));
        if [a, b].iter().all(|v| w.vehicles.cars[*v].spot().is_some_and(|s| s.kind == ParkKind::Pound)) {
            break;
        }
    }
    assert!(w.enforcement.stats.u_turns_total >= 1, "a U-turn in the street");
    assert!(turning, "seen turning round");
    for v in [a, b] {
        assert_eq!(w.vehicles.cars[v].spot().map(|s| s.kind), Some(ParkKind::Pound));
    }
    assert!(w.driving.moves.is_empty());
    assert_no_car_lost(&w);
}

/// [tow-moves] Same city, same tows, same moves.
#[test]
fn tow_moves_are_deterministic() {
    let make = || {
        let mut w = town();
        w.add_road(&[Vec2::new(300.0, -200.0), Vec2::new(300.0, 0.0)], 1, 0);
        w.add_road(&[Vec2::new(0.0, -200.0), Vec2::new(300.0, -200.0)], 1, 1);
        run(&mut w, 2);
        let (_, v) = car_near(&mut w, Vec2::new(300.0, -45.0));
        w.call_tow(v, TowReason::Illegal);
        run(&mut w, 120);
        w
    };
    let (a, b) = (make(), make());
    let poses = |w: &World| w.traffic.cars.iter().map(|(id, c)| format!("{id:?} {:.3} {:.3} {:.3}", c.pos.x, c.pos.y, c.heading)).collect::<Vec<_>>();
    assert_eq!(poses(&a), poses(&b));
    assert_eq!(format!("{:?}", a.enforcement.stats), format!("{:?}", b.enforcement.stats));
}

#[test]
fn a_road_stops_at_a_parking_lot_the_player_placed() {
    let mut w = town();
    let lot = w.lots.iter().filter(|(_, l)| l.usable() && l.car_access() && l.land_use != LandUse::Park && l.area() >= LotSize::Small.min_area()).map(|(id, _)| id).next().expect("a lot");
    let b = w.place_on_lot(crate::services::Placeable::ParkingLot(LotSize::Small), lot).expect("placed");
    assert!(w.buildings[b].city_parking && World::is_civic_structure(&w.buildings[b]));
    let lot = w.buildings[b].lot;
    let c = w.lots[lot].center;
    let pts = [c - Vec2::new(40.0, 0.0), c + Vec2::new(40.0, 0.0)];
    assert!(w.structures_in_way(&pts, &[0.0, 0.0], 1, 1, crate::roads::RoadKind::Street).contains(&b), "the player's lot blocks a road through it");
    // A save from before the flag: the city's lots learn it on load.
    w.buildings[b].city_parking = false;
    let w = World::from_ron(&w.to_ron().expect("saves")).expect("loads");
    assert!(w.buildings[b].city_parking);
}

#[test]
fn big_vehicles_get_a_heavy_tow_truck() {
    // [tow] Lorries, buses and RVs are too big for a wheel lift.
    use crate::vehicle::BodyType;
    assert_eq!(super::enforcement::tow_model_for(BodyType::Sedan).body, BodyType::TowTruck);
    assert_eq!(super::enforcement::tow_model_for(BodyType::Hatchback).body, BodyType::TowTruck);
    for big in [BodyType::Semi, BodyType::BubbleBus, BodyType::PanelTruck, BodyType::GarbageTruck] {
        assert_eq!(super::enforcement::tow_model_for(big).body, BodyType::HeavyTow, "{big:?}");
    }
}

/// [tow-fix] G10/G15/G19/G20: a tow truck turning round in a street with
/// a highway beside (or over) it came back onto the lanes of whatever
/// "street" was nearest, the highway, where no road led to its yard; it
/// pulled in there to try again, and again, for hours. It now comes back
/// onto the street it turned in, or the nearest ordinary street.
#[test]
fn a_truck_off_the_lanes_comes_back_onto_its_street_not_a_highway_beside_it() {
    let mut w = town();
    // A highway close beside the long south street.
    w.add_road(&[Vec2::new(16.0, -290.0), Vec2::new(16.0, -60.0)], 3, 0);
    run(&mut w, 2);
    let at = Vec2::new(10.0, -160.0);
    let (hw, _, _) = w.roads.nearest_street(at).unwrap();
    assert!(w.roads.segment(hw).is_highway(), "the trap: the highway is the nearest street there");
    let street = w.roads.nearest_where(Vec2::new(0.0, -160.0), |s| s.is_street() && !s.is_highway()).map(|(s, _, _)| s).unwrap();
    assert_eq!(w.street_under(at, None), Some(street));
    assert_eq!(w.street_under(at, Some(street)), Some(street));
    // A truck that turned round there and ends up nearer the highway.
    let (lane, s, _) = w.traffic.net.nearest_lane_on(street, Vec2::new(0.0, -160.0)).unwrap();
    let truck = w.traffic.spawn_car(CitizenId::default(), None, vec![lane], s, s + 3.0, 0);
    w.traffic.begin_manoeuvre(truck, vec![at], 0.0, 2.0);
    w.traffic.cars[truck].pos = at;
    w.back_on_lanes(truck, None, None);
    let c = &w.traffic.cars[truck];
    assert!(!c.in_stunt());
    match &w.traffic.net.lanes[c.route[c.idx] as usize].kind {
        crate::traffic::LaneKind::Road { segment, .. } => assert_eq!(*segment, street, "back on the street it turned in"),
        k => panic!("on {k:?}"),
    }
}

/// [tow-fix] A truck with a car on and no road from where it stands to its
/// yard (a highway that only leads out of town) takes the car to the pound
/// out of town, rather than pulling in on the carriageway to try again for
/// ever.
#[test]
fn a_tow_truck_with_no_road_to_its_yard_takes_the_car_out_of_town() {
    let mut w = town();
    let yard = tow_yard(&mut w);
    // A highway on its own, leading off the map.
    w.add_road(&[Vec2::new(-300.0, -500.0), Vec2::new(crate::landscape::LAND_HALF, -500.0)], 3, 0);
    run(&mut w, 2);
    let (_, vid) = car_near(&mut w, Vec2::new(4.0, -150.0));
    let job = w.call_tow(vid, TowReason::Illegal).expect("tow called");
    // The truck stands on the highway, the car's tow job its errand.
    let (hw, _, _) = w.roads.nearest_street(Vec2::new(0.0, -500.0)).unwrap();
    let (lane, s, _) = w.traffic.net.nearest_lane_on(hw, Vec2::new(0.0, -500.0)).unwrap();
    let truck = w.traffic.spawn_car(CitizenId::default(), None, vec![lane], s, s + 2.0, 0);
    w.traffic.cars[truck].errand = Errand::Tow(job);
    if let Some(j) = w.enforcement.jobs.iter_mut().find(|j| j.id == job) {
        j.company = Some(yard);
        j.truck = Some(truck);
        j.state = TowState::ToCar;
    }
    w.tow_truck_arrived(truck, job);
    assert_eq!(w.enforcement.job(job).map(|j| j.state), Some(TowState::ToPound), "hooked");
    assert_eq!(w.enforcement.stats.pound_detours_total, 1, "no way to the yard: out of town instead");
    assert_eq!(w.enforcement.job(job).and_then(|j| j.company), None);
    assert!(w.traffic.cars[truck].hold <= 0.0, "driving on, not pulled in on the carriageway");
    for _ in 0..600 {
        run(&mut w, 1);
        if w.vehicles.cars[vid].spot().is_some_and(|s| s.kind == ParkKind::Pound) {
            break;
        }
    }
    let spot = w.vehicles.cars[vid].spot().copied().unwrap();
    assert_eq!((spot.kind, spot.building), (ParkKind::Pound, None), "in the pound out of town");
    assert!(!w.traffic.cars.contains_key(truck));
}

/// [watchdog] A service vehicle standing still on a carriageway for a
/// reason of its own (pulled in to try again) is sent on after
/// `STILL_REROUTE` minutes and taken off the road after `STILL_CLEAR`.
#[test]
fn the_watchdog_clears_a_service_vehicle_standing_in_the_road() {
    use super::watchdog::{STILL_CLEAR, STILL_REROUTE};
    let mut w = town();
    let (seg, _, _) = w.roads.nearest_street(Vec2::new(0.0, -200.0)).unwrap();
    let (lane, s, _) = w.traffic.net.nearest_lane_on(seg, Vec2::new(0.0, -200.0)).unwrap();
    let truck = w.traffic.spawn_car(CitizenId::default(), None, vec![lane], s, s + 2.0, 0);
    w.traffic.cars[truck].errand = Errand::Out;
    // Broken down for good: whatever it is told, it stays where it stands.
    let mut held = |w: &mut World, n: u64| {
        for _ in 0..n {
            if w.traffic.cars.contains_key(truck) {
                w.traffic.pull_in(truck, 1.0e9);
            }
            run(w, 1);
        }
    };
    held(&mut w, STILL_REROUTE + 10);
    assert!(w.traffic.cars.contains_key(truck));
    assert!(w.driving.watchdog_rerouted_total >= 1, "sent on first");
    assert_eq!(w.traffic.cars[truck].errand, Errand::Out, "off the map, with nowhere else to go");
    held(&mut w, STILL_CLEAR - STILL_REROUTE + 10);
    assert!(!w.traffic.cars.contains_key(truck), "then cleared off the road");
    assert_eq!(w.driving.watchdog_cleared_total, 1);
}

/// [airlift] G19: the airlift. The helicopter is paid for, flies in, the
/// vehicle stays put until it is over it, is lifted off the road (a stuck
/// tow truck's car going to the pound), and the helicopter flies away.
#[test]
fn the_airlift_lifts_a_stuck_tow_truck_and_its_car_away() {
    use super::airlift::*;
    let mut w = town();
    let yard = tow_yard(&mut w);
    let (_, vid) = car_near(&mut w, Vec2::new(4.0, -150.0));
    let job = w.call_tow(vid, TowReason::Illegal).expect("tow called");
    let (seg, _, _) = w.roads.nearest_street(Vec2::new(0.0, -200.0)).unwrap();
    let (lane, s, _) = w.traffic.net.nearest_lane_on(seg, Vec2::new(0.0, -200.0)).unwrap();
    let truck = w.traffic.spawn_car(CitizenId::default(), None, vec![lane], s, s + 2.0, 0);
    w.traffic.cars[truck].errand = Errand::Tow(job);
    if let Some(j) = w.enforcement.jobs.iter_mut().find(|j| j.id == job) {
        j.company = Some(yard);
        j.truck = Some(truck);
        j.state = TowState::ToCar;
    }
    assert!(w.hook_parked(truck, job));
    w.traffic.pull_in(truck, 1.0e9);
    let before = w.economy.treasury;
    w.airlift(truck).expect("airlift called");
    assert!(w.airlift(truck).is_err(), "once is enough");
    assert!((before - w.economy.treasury - AIRLIFT_COST).abs() < 0.01, "paid for");
    run(&mut w, (AIRLIFT_INBOUND - 1) as usize);
    assert!(w.traffic.cars.contains_key(truck), "still there until the helicopter is over it");
    run(&mut w, AIRLIFT_HOOK as usize + 1);
    assert!(!w.traffic.cars.contains_key(truck), "lifted off the road");
    assert!(w.enforcement.job(job).is_none(), "the job is done");
    let spot = w.vehicles.cars[vid].spot().copied().unwrap();
    assert_eq!((spot.kind, spot.building), (ParkKind::Pound, Some(yard)), "its car in the yard's pound");
    assert_eq!(w.airlifts().len(), 1, "flying away with it");
    run(&mut w, AIRLIFT_OUTBOUND as usize + 1);
    assert!(w.airlifts().is_empty());
    assert_no_car_lost(&w);
    // A bus is refused, and so is an empty treasury.
    w.economy.treasury = 10.0;
    let (lane2, s2, _) = w.traffic.net.nearest_lane_on(seg, Vec2::new(0.0, -120.0)).unwrap();
    let other = w.traffic.spawn_car(CitizenId::default(), None, vec![lane2], s2, s2 + 2.0, 0);
    assert!(w.airlift(other).unwrap_err().contains("treasury"));
}

// ── [r2-traffic] ──────────────────────────────────────────────────────────

#[test]
fn a_parked_car_keeps_its_kerb_slot_when_the_ids_change() {
    // K41: the road ids renumbered by a rebuild left every parked car
    // pointing at another street; the kerb index then marked slots taken on
    // whatever streets had those numbers and cars were moved, one slot
    // free after another, onto a road just built. Found by where it stands,
    // the car keeps its slot.
    let mut w = town();
    let (_, vid) = parked_far(&mut w);
    let spot = *w.vehicles.cars[vid].spot().unwrap();
    let other = w.roads.nearest_street(Vec2::new(-300.0, 0.0)).unwrap().0;
    assert_ne!(Some(other), spot.segment);
    if let CarPlace::Parked(s) = &mut w.vehicles.cars[vid].place {
        (s.segment, s.slot) = (Some(other), 0);
    }
    w.remap_kerb_slots();
    let now = *w.vehicles.cars[vid].spot().unwrap();
    assert_eq!((now.segment, now.side, now.slot), (spot.segment, spot.side, spot.slot), "its own slot again");
    assert_eq!(now.pos, spot.pos, "and it never moved");
    assert_eq!(w.vehicles.kerbs[&spot.segment.unwrap()].get(&(spot.side, spot.slot)), Some(&vid));
}

#[test]
fn a_car_left_with_no_slot_stands_at_the_kerb_not_in_the_lane() {
    // K32/K35: a car squeezed in where there was no slot kept its place in
    // its lane and was drawn in the middle of the road.
    let w = town();
    let lane_centre = Vec2::new(1.75, -150.0);
    let spot = w.squeezed_spot(lane_centre);
    let half = w.roads.segment(spot.segment.expect("on the street")).half_width();
    assert!((spot.pos.x.abs() - (half - 1.15)).abs() < 0.05, "on the kerb line: {:?} (half width {half})", spot.pos);
    assert!(spot.pos.x > 0.0, "on the side it stood");
    assert!((spot.pos.y + 150.0).abs() < 0.5);
}

#[test]
fn tow_calls_lapse_and_are_not_booked_past_what_the_trucks_can_take() {
    // The tow backlog: every illegally parked car a patrol passed became a
    // tow call at once and none ever lapsed; Harvey's city had 1,341 (and
    // later 2,210) queued for a handful of trucks.
    let mut w = town();
    let (_, vid) = parked_far(&mut w);
    let tick = w.clock.tick;
    let trucks: usize = w.buildings.keys().map(|b| w.tow_trucks(b)).sum::<usize>() + OUT_OF_TOWN_TOW_TRUCKS;
    for k in 0..trucks * TOW_QUEUE_PER_TRUCK {
        w.enforcement.jobs.push(TowJob { id: 10_000 + k as u32, car: VehicleId::default(), company: None, truck: None, state: TowState::Waiting { until: tick + 600 }, reason: TowReason::Illegal, since: tick, contra: None });
    }
    w.call_tow_if_room(vid);
    assert!(w.enforcement.job_for(vid).is_none(), "the queue is full: a ticket only");
    assert_eq!(w.enforcement.stats.tows_declined_total, 1);
    w.enforcement.jobs.clear();
    w.call_tow_if_room(vid);
    let id = w.enforcement.job_for(vid).expect("room again: towed").id;
    // Nobody comes for hours: the call lapses.
    if let Some(j) = w.enforcement.jobs.iter_mut().find(|j| j.id == id) {
        j.since = tick.saturating_sub(TOW_LAPSE + 1);
        j.state = TowState::Waiting { until: tick + 600 };
    }
    run(&mut w, 2);
    assert!(w.enforcement.job_for(vid).is_none(), "lapsed");
    assert_eq!(w.enforcement.stats.tows_lapsed_total, 1);
}

#[test]
fn a_car_park_is_redrawn_for_every_car_while_few_stand_there() {
    // K44: "2/109 spots taken" on a lot drawn empty: it was redrawn only
    // when its fullness crossed an eighth.
    use super::cars::parking_band;
    assert_ne!(parking_band(0, 109), parking_band(2, 109));
    assert!((0..=12).all(|n| parking_band(n, 109) != parking_band(n + 1, 109)));
    assert_eq!(parking_band(60, 109), parking_band(61, 109), "a full lot is not redrawn for every car");
}

/// [r1-roads] (K42) Harvey's three car parks side by side: City Parking 20
/// and City Parking 50 (itself two plots joined) meet along an edge that is
/// not quite one line (a corner 0.3 m off it). The union left a sliver and
/// they never joined; the plots are now put on one line first.
#[test]
fn car_parks_meeting_along_a_skewed_edge_join() {
    let v = |pts: &[(f32, f32)]| pts.iter().map(|&(x, y)| Vec2::new(x, y)).collect::<Vec<_>>();
    let cp20 = v(&[(152.0, -189.0), (171.0, -217.0), (204.0, -194.0), (185.0, -166.0)]);
    let cp50 = v(&[(137.0, -199.0), (185.0, -166.0), (148.0, -113.0), (152.0, -43.0), (137.0, -53.0)]);
    let joined = super::parking::join_plots(&cp50, &cp20).expect("they join into one piece");
    let sum = crate::geom::polygon_area(&cp20).abs() + crate::geom::polygon_area(&cp50).abs();
    let area = crate::geom::polygon_area(&joined).abs();
    assert!((area - sum).abs() < sum * 0.02, "the joined lot has both plots' ground: {area:.0} of {sum:.0}");
    // Plots well apart still do not join.
    let far = v(&[(300.0, -189.0), (319.0, -217.0), (352.0, -194.0), (333.0, -166.0)]);
    assert!(super::parking::join_plots(&cp50, &far).is_none());
}

// ── [r7-citizen-story] the pound fetch, the travel history, home parking ──

/// Run until the clock reads `hour` (on the next day if it is past it).
fn run_to_hour(w: &mut World, hour: u64) {
    while w.clock.hour() != hour {
        run(w, 1);
    }
}

#[test]
fn the_pound_memory_says_what_it_costs_now_and_that_storage_grows() {
    let m = super::enforcement::pound_memory("Hook & Haul", TOW_FEE + STORAGE_PER_DAY);
    assert_eq!(m, "The car's in the pound at Hook & Haul. 95 to get it out now, 15 more each day it stays.");
    let r = super::enforcement::release_memory("Hook & Haul", 110.0, TOW_FEE, 2);
    assert_eq!(r, "Paid 110 to get the car out of the pound at Hook & Haul (80 for the tow, 30 for 2 days of storage).");
}

/// Harvey's Yuki Diallo: towed in the evening, so nothing can happen
/// before 08:00; then the owner goes to the pound (one trip), pays and
/// drives home (the next), and the travel history and the memories say
/// so in that order, with the right modes, through a save.
#[test]
fn a_car_towed_at_night_is_fetched_after_eight_and_the_history_shows_it() {
    let mut w = town();
    let yard = tow_yard(&mut w);
    run_to_hour(&mut w, 21);
    let (id, vid) = parked_far(&mut w);
    if let Some(h) = w.citizens[id].household {
        w.households[h].money = 1000.0;
    }
    w.order_tow(vid).expect("clamped and towed");
    for _ in 0..600 {
        run(&mut w, 1);
        if w.vehicles.cars[vid].spot().is_some_and(|s| s.kind == ParkKind::Pound) {
            break;
        }
    }
    let spot = *w.vehicles.cars[vid].spot().unwrap();
    assert_eq!((spot.kind, spot.building), (ParkKind::Pound, Some(yard)), "in the yard's pound by night");
    let towed_day = spot.since / 1440;
    let memory = w.citizens[id].memories.iter().rev().find(|m| m.text.contains("in the pound")).map(|m| m.text.clone()).expect("remembers the tow");
    assert!(memory.contains("95 to get it out now, 15 more each day it stays"), "{memory}");
    // Nothing before 08:00 the next morning.
    while !(w.clock.day() > towed_day && w.clock.hour() >= 8) {
        run(&mut w, 1);
        assert_eq!(w.enforcement.stats.released_total, 0, "fetched at {} before 08:00", w.clock.format_hhmm());
    }
    w.citizens[id].next_decision_tick = u64::MAX;
    for _ in 0..12 * 60 {
        run(&mut w, 1);
        if w.enforcement.stats.released_total > 0 && !w.citizens[id].activity.is_traveling() {
            break;
        }
    }
    assert_eq!(w.enforcement.stats.released_total, 1, "fetched the next day");
    let paid = w.citizens[id].memories.iter().find(|m| m.text.starts_with("Paid") && m.text.contains("pound")).expect("remembers paying");
    assert!(paid.tick % 1440 >= 8 * 60, "paid at {}", paid.tick % 1440);
    assert!(paid.text.contains("(80 for the tow, 15 for 1 day of storage)") || paid.text.contains("days of storage"), "{}", paid.text);
    let trips = w.travel_history(id, 2);
    let to_pound = trips.iter().position(|t| t.why == "to get the car out of the pound" && t.to == Some(yard)).unwrap_or_else(|| panic!("the trip to the pound: {:#?}", trips));
    let t = &trips[to_pound];
    assert!(t.minutes.is_some() && !t.modes.contains(&"car"), "a trip of its own, not by car: {t:?}");
    assert!(t.depart % 1440 >= 8 * 60, "set off after 08:00: {t:?}");
    let back = trips.get(to_pound + 1).unwrap_or_else(|| panic!("the drive home: {:#?}", trips));
    assert_eq!(back.from, Some(yard));
    assert_eq!(back.modes, vec!["car"], "{back:?}");
    assert!(back.events.iter().any(|e| e.starts_with("paid") && e.contains("pound")), "{back:?}");
    // Saved and loaded: the same history.
    let again = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(again.travel_history(id, 2), trips);
    assert_no_car_lost(&w);
}

/// A car left far from home (a move, a trip that ended without it) is
/// fetched by day and parked where the residents' permit covers it, not
/// left on a two-hour kerb to be ticketed, clamped and towed.
#[test]
fn an_owner_fetches_a_car_left_far_from_home_and_parks_it_legally() {
    let mut w = town();
    let id = w.citizens.iter().filter(|(_, c)| c.home.is_some() && c.can_work()).map(|(id, _)| id).next().unwrap();
    if let Some(v) = w.citizens[id].car {
        w.drop_car(v, CarEvent::Sold);
    }
    let home = w.citizens[id].home.unwrap();
    let door = w.building_entrance(home).unwrap();
    // No kerb by these homes: spaces of their own (a mid-rise's few).
    w.buildings[home].parking_capacity = w.buildings[home].parking_used + 2;
    let vid = w.give_car_parked(id, Vec2::new(-390.0, 0.0));
    let at = w.vehicles.cars[vid].spot().unwrap().pos;
    assert!(at.distance(door) > super::cars::CAR_REACH, "out of reach of home: {}", at.distance(door));
    assert!(w.kerb_limit(vid).is_some(), "a time limit where it stands");
    // At home and awake from 17:00 (owners with a job go after work; they
    // set off from the hourly check at :47).
    run_to_hour(&mut w, 17);
    {
        let c = &mut w.citizens[id];
        c.activity = Activity::AtHome;
        c.inside = Some(home);
        c.asleep = false;
        c.wakefulness = 1.0;
        c.next_decision_tick = u64::MAX;
    }
    let mut set_off = false;
    for _ in 0..8 * 60 {
        run(&mut w, 1);
        set_off |= w.citizens[id].to_car.is_some();
        if set_off && w.vehicles.cars[vid].spot().is_some_and(|s| s.pos.distance(door) < PERMIT_RADIUS) && !w.citizens[id].activity.is_traveling() {
            break;
        }
    }
    assert!(set_off, "went to get it");
    let spot = *w.vehicles.cars[vid].spot().expect("parked again");
    assert!(spot.pos.distance(door) < PERMIT_RADIUS, "back by home: {:?} {}", spot.kind, spot.pos.distance(door));
    assert_eq!(w.kerb_limit(vid), None, "a legal spot: a home space or the residents' permit");
    let trips = w.travel_history(id, 1);
    let fetch = trips.iter().find(|t| t.why == "to fetch the car").unwrap_or_else(|| panic!("{trips:#?}"));
    assert!(fetch.modes.contains(&"walk") && fetch.modes.contains(&"car"), "walked there, drove back: {fetch:?}");
    assert!(w.car_whereabouts(id).is_some_and(|s| !s.contains("limit") || s.contains("no limit")), "{:?}", w.car_whereabouts(id));
}

/// [r7-citizen-story] A park has no garage: nobody parks in it out of sight.
#[test]
fn a_park_has_no_hidden_parking_spaces() {
    let mut w = town();
    w.add_zone(square(10.0, -200.0, 150.0), LandUse::Park);
    run(&mut w, 6 * 60);
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
    run(&mut w, 60);
    let parks: Vec<&crate::buildings::Building> = w.buildings.values().filter(|b| b.land_use == LandUse::Park).collect();
    assert!(!parks.is_empty(), "a park was laid out");
    assert!(parks.iter().all(|b| b.parking_capacity == 0), "{:?}", parks.iter().map(|b| b.parking_capacity).collect::<Vec<_>>());
}
