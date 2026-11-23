//! [K34] Shipping containers' lives: in by rail, trucked to the buyer and
//! stored empty; bought off the lorry by a stack developer and built into a
//! home; scrapped into steel; kept through a save; and an older save given
//! a starting stock.

use super::*;
use crate::containers::{self as cx, Event, Owner, Where};
use crate::mind::IdleMind;
use crate::rail::{TrainKind, TRUNK_Y};
use super::containers::DepotKind;

fn tick(w: &mut World, n: u32) {
    let mut minds = IdleMind;
    for _ in 0..n {
        w.tick(&mut minds);
    }
}

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

/// A freight yard by the trunk line, a street along it, an office (the
/// buyer) and industrial land across the street.
fn town() -> (World, BuildingId, BuildingId) {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, ..WorldConfig::test_town() });
    w.economy.treasury = 1_000_000.0;
    let y = TRUNK_Y + 120.0;
    w.add_road(&[Vec2::new(-500.0, y), Vec2::new(500.0, y)], 1, 1);
    let yard = w.place_rail_facility(RailFacility::FreightYard, Vec2::new(0.0, TRUNK_Y + 40.0)).expect("a yard by the trunk");
    let end = w.facilities.values().find(|f| f.building == Some(yard)).unwrap().road_line()[1];
    w.add_road(&[end, Vec2::new(end.x, y)], 1, 1);
    w.add_zone(square(-400.0, y + 10.0, 300.0, 60.0), LandUse::Commercial);
    w.add_zone(square(100.0, y + 10.0, 350.0, 90.0), LandUse::Industrial);
    tick(&mut w, 2);
    assert!(w.buildings[yard].rail.as_ref().unwrap().works());
    let shop = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Commercial && l.car_access() && l.building.is_none()).map(|(id, _)| id).next().unwrap();
    w.build_on(shop);
    let b = w.lots[shop].building.unwrap();
    w.finish_construction(b);
    let jobs = w.buildings[b].jobs;
    w.buildings[b].business = Some(Business::new(BusinessKind::Office, jobs));
    (w, yard, b)
}

fn free_lot(w: &World, land: LandUse) -> LotId {
    w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == land && l.car_access() && l.building.is_none()).max_by(|a, b| a.1.area().total_cmp(&b.1.area()).then(b.0.cmp(&a.0))).map(|(id, _)| id).expect("a free lot")
}

fn valid_number(text: &str) -> bool {
    let c: Vec<u8> = text.bytes().filter(|b| b.is_ascii_alphanumeric()).collect();
    c.len() == 11 && c[3] == b'U' && cx::check_digit(&[c[0], c[1], c[2], c[3]], std::str::from_utf8(&c[4..10]).unwrap().parse().unwrap()) == c[10] - b'0'
}

fn events(w: &World, id: cx::BoxId) -> Vec<Event> {
    w.containers.boxes[id].log.iter().map(|s| s.1).collect()
}

/// A box comes in full on a freight train, is set down at the yard,
/// trucked to its buyer, emptied there and taken back into storage, with
/// every step in its history and a valid number on its side.
#[test]
fn a_box_comes_by_rail_is_trucked_to_its_buyer_and_stored_empty() {
    let (mut w, yard, buyer) = town();
    w.rail.orders.push(crate::rail::RailOrder { cargo: Cargo { from: yard, to: buyer, resource: Resource::Parts, amount: 20.0, price: 10.0 }, since: 0 });
    w.note_inbound(buyer, Resource::Parts, 20.0);
    assert!(w.spawn_train(TrainKind::Freight, &[yard]));
    let t = w.rail.trains.keys().next().unwrap();
    let id = w.rail.trains[t].boxes[0].id.expect("the box on the wagon has a record");
    assert_eq!(w.containers.boxes[id].at, Where::Train);
    assert!(valid_number(&w.containers.boxes[id].number()), "{}", w.containers.boxes[id].number());
    let stock0 = w.buildings[buyer].business.as_ref().unwrap().stock_of(Resource::Parts);
    let mut saw = (false, false, false);
    for _ in 0..4000 {
        tick(&mut w, 1);
        let Some(r) = w.containers.boxes.get(id) else { panic!("the box vanished: {:?}", w.containers.stats) };
        match r.at {
            Where::Stored(b) if b == yard && r.goods.is_some() => saw.0 = true,
            Where::Truck(_) if r.goods.is_some() => saw.1 = true,
            _ => {}
        }
        if r.log.iter().any(|s| s.1 == Event::Delivered && s.2 == buyer) {
            saw.2 = true;
        }
        if saw.2 && matches!(r.at, Where::Stored(_)) {
            break;
        }
    }
    assert!(saw.0 && saw.1 && saw.2, "set down at the yard, on a lorry, delivered: {saw:?}, {:?}", events(&w, id));
    let r = &w.containers.boxes[id];
    assert!(r.goods.is_none(), "emptied");
    assert_eq!(r.at, Where::Stored(yard), "back in storage at the yard: {:?}", events(&w, id));
    let ev = events(&w, id);
    assert_eq!(ev[0], Event::ArrivedRail);
    // [r8] Lifted onto the lorry's chassis at the yard, and off it again
    // into the stack when the lorry came back with it empty.
    assert!(ev.contains(&Event::CraneOn) && ev.contains(&Event::CraneOff), "{ev:?}");
    assert!(w.buildings[buyer].business.as_ref().unwrap().stock_of(Resource::Parts) >= stock0 + 20.0 - 0.01, "the goods arrived");
    assert!(w.containers.stats.delivered_total >= 1 && w.containers.stats.arrived_total[0] >= 1);
    let story = w.box_story(id).unwrap();
    assert!(story.history.iter().any(|h| h.1.contains("delivered")), "{:?}", story.history);
}

/// A stack developer's site nearby buys the box straight off the lorry
/// that has just emptied it there; the lorry takes it to the site; the
/// finished home is built from it (and boxes bought out of town for the
/// rest), and the inspector says so.
#[test]
fn a_stack_developer_buys_a_box_off_the_truck_and_builds_a_home_of_it() {
    let (mut w, yard, buyer) = town();
    // A home going up on the commercial strip's next lot, taken on by a
    // stack developer.
    let lot = free_lot(&w, LandUse::Commercial);
    w.build_on(lot);
    let site = w.lots[lot].building.unwrap();
    assert!(w.buildings[site].construction.is_some());
    w.containers.homes.insert(site, cx::BoxHome { want: 6, boxes: Vec::new(), done: false });
    // A full box at the yard, trucked to the buyer.
    let id = w.make_box(crate::rail::BoxKind::Dry, false, Where::Stored(yard), Some(cx::Goods { resource: Resource::Parts, buyer, amount: 0.0 }), Event::ArrivedRail, yard);
    w.containers.boxes[id].built = w.clock.day() as i32 - 365 * 3;
    w.containers.boxes[id].knocks = 0;
    w.note_inbound(buyer, Resource::Parts, 20.0);
    w.dispatch_truck(super::market::TruckOrigin::Building(yard), Cargo { from: yard, to: buyer, resource: Resource::Parts, amount: 20.0, price: 0.0 }, 6);
    // [r8] A lorry pulls into a bay and the carrier lifts the box onto it.
    let mut car = None;
    for _ in 0..60 {
        if let Where::Truck(c) = w.containers.boxes[id].at {
            car = Some(c);
            break;
        }
        tick(&mut w, 1);
    }
    let car = car.unwrap_or_else(|| panic!("the box rides on the lorry: {:?}", w.containers.boxes[id].at));
    assert_eq!(w.traffic.cars[car].model.body, crate::vehicle::BodyType::Semi);
    assert!(w.traffic.cars[car].model.boxed > 0, "the lorry wears the box");
    let mut bought = false;
    for _ in 0..3000 {
        tick(&mut w, 1);
        let r = &w.containers.boxes[id];
        if r.owner == Owner::Home(site) {
            bought = true;
        }
        if r.at == Where::Site(site) {
            break;
        }
    }
    let r = &w.containers.boxes[id];
    assert!(bought, "the developer bought it: {:?}", events(&w, id));
    assert_eq!(r.at, Where::Site(site), "the lorry took it to the site: {:?}", events(&w, id));
    assert!(events(&w, id).contains(&Event::BoughtOffTruck));
    // (Lorries bringing the town's other imports may sell it more.)
    assert!(w.containers.stats.sold_off_truck_total >= 1);
    w.finish_construction(site);
    let h = &w.containers.homes[&site];
    assert!(h.done && h.boxes.len() == 6 && h.boxes.contains(&id), "{h:?}");
    assert!(h.boxes.iter().all(|b| w.containers.boxes[*b].at == Where::Home(site)));
    let bought_here = h.boxes.iter().filter(|b| w.containers.boxes[**b].log.iter().any(|s| matches!(s.1, Event::BoughtOffTruck | Event::SoldToDeveloper))).count() as u32;
    assert!(bought_here >= 1);
    assert_eq!(w.containers.stats.bought_out_of_town_for_homes, 6 - bought_here, "the rest bought out of town");
    let faces = w.containers.home_faces(site, w.clock.day() as i64);
    assert_eq!(faces.len(), 6);
    assert!(faces.iter().any(|f| f.id == id));
    assert!(w.box_story(id).unwrap().now.contains("part of the home"));
}

/// An old box at a yard is taken to the scrap yard and cut up there: the
/// steel goes into the scrap yard's stock, which building sites then buy.
#[test]
fn worn_out_boxes_are_scrapped_into_steel() {
    let (mut w, yard, _) = town();
    let lot = free_lot(&w, LandUse::Industrial);
    let scrap = w.open_business(lot, BusinessKind::ScrapYard, true).expect("a scrap yard");
    assert_eq!(w.box_depot(scrap), Some(DepotKind::Scrap));
    let old = w.make_box(crate::rail::BoxKind::Dry, true, Where::Stored(yard), None, Event::ArrivedRail, yard);
    w.containers.boxes[old].built = w.clock.day() as i32 - 365 * 22;
    let young = w.make_box(crate::rail::BoxKind::Dry, false, Where::Stored(yard), None, Event::ArrivedRail, yard);
    w.containers.boxes[young].built = w.clock.day() as i32 - 200;
    w.containers.boxes[young].knocks = 0;
    let day = w.clock.day() as i64;
    assert!(w.containers.boxes[old].for_scrap(day) && !w.containers.boxes[young].for_scrap(day));
    let steel0 = w.buildings[scrap].business.as_ref().unwrap().stock_of(Resource::Metal);
    w.daily_containers();
    assert_eq!(w.containers.boxes[old].owner, Owner::Scrap(scrap));
    for _ in 0..3000 {
        tick(&mut w, 1);
        if !w.containers.boxes.contains_key(old) {
            break;
        }
    }
    assert!(!w.containers.boxes.contains_key(old), "cut up");
    assert!(w.containers.boxes.contains_key(young), "the sound box is kept");
    assert_eq!(w.containers.stats.scrapped_total, 1);
    assert!((w.containers.stats.steel_total - cx::STEEL_40).abs() < 1e-3);
    let steel = w.buildings[scrap].business.as_ref().unwrap().stock_of(Resource::Metal);
    assert!(steel >= steel0 + cx::STEEL_40 - 0.5, "steel in stock: {steel0} -> {steel}");
    // A building site near it takes its steel.
    w.buildings[scrap].business.as_mut().unwrap().add(Resource::Metal, 30.0);
    let site_lot = free_lot(&w, LandUse::Commercial);
    let before = w.buildings[scrap].business.as_ref().unwrap().stock_of(Resource::Metal);
    // [r14-materials] Breaking ground orders the foundations (their rebar).
    w.build_on(site_lot);
    let site = w.lots[site_lot].building.unwrap();
    w.order_materials(site);
    assert!(w.buildings[scrap].business.as_ref().unwrap().stock_of(Resource::Metal) < before, "the site bought scrap steel");
}

/// Old boxes with no scrap yard in town press for one.
#[test]
fn old_boxes_with_no_scrap_yard_ask_for_one() {
    let (mut w, yard, _) = town();
    for _ in 0..30 {
        let id = w.make_box(crate::rail::BoxKind::Dry, false, Where::Stored(yard), None, Event::ArrivedRail, yard);
        w.containers.boxes[id].built = -365 * 25;
    }
    w.daily_containers();
    assert_eq!(w.containers.stats.due_scrap_today, 30);
    let p = crate::pressure::assess(&crate::pressure::Signals { old_boxes: w.containers.stats.due_scrap_today as f32, ..Default::default() }, &w.pressure_supply());
    let scrap = p.iter().find(|p| p.kind == crate::pressure::PressureKind::ScrapYard).unwrap();
    assert!(scrap.wanted >= 2.0, "{scrap:?}");
}

/// The records survive a save: the same boxes, numbers, places and homes.
#[test]
fn boxes_survive_a_save_and_load() {
    let (mut w, yard, _) = town();
    for k in 0..5 {
        w.make_box(if k == 0 { crate::rail::BoxKind::Reefer } else { crate::rail::BoxKind::Dry }, k % 2 == 0, Where::Stored(yard), None, Event::ArrivedSea, yard);
    }
    let before: Vec<(String, Where, Vec<cx::Stop>)> = w.containers.boxes.values().map(|r| (r.number(), r.at, r.log.clone())).collect();
    let ron = w.to_ron().unwrap();
    let w2 = World::from_ron(&ron).unwrap();
    assert!(w2.containers.live);
    let after: Vec<(String, Where, Vec<cx::Stop>)> = w2.containers.boxes.values().map(|r| (r.number(), r.at, r.log.clone())).collect();
    assert_eq!(before, after);
    assert_eq!(w.containers.stats, w2.containers.stats);
}

/// A save from before the records loads with a plausible stock of boxes
/// at its depots: mixed lines and ages, every one with a valid number.
#[test]
fn an_older_save_gets_a_starting_stock() {
    let (mut w, yard, _) = town();
    let lot = free_lot(&w, LandUse::Industrial);
    let cy = w.open_business(lot, BusinessKind::ContainerYard, true).expect("a container yard");
    w.containers = Default::default();
    let ron = w.to_ron().unwrap();
    // Cut the field out altogether, as an older save has it.
    let cut = ron.find("\n    containers: (").expect("the field is saved");
    let old = format!("{}\n)", &ron[..cut]);
    let w2 = World::from_ron(&old).expect("an older save loads");
    assert!(w2.containers.live);
    let at_yard = w2.containers.stored_at(cy).len();
    let at_rail = w2.containers.stored_at(yard).len();
    assert!(at_yard >= 5 && at_rail >= 5, "stock: {at_yard} at the container yard, {at_rail} at the freight yard");
    let lines: std::collections::BTreeSet<u8> = w2.containers.boxes.values().map(|r| r.line).collect();
    assert!(lines.len() >= 4, "mixed lines: {lines:?}");
    let day = w2.clock.day() as i64;
    let worn = w2.containers.boxes.values().filter(|r| r.wear(day) > 0.4).count();
    let fresh = w2.containers.boxes.values().filter(|r| r.wear(day) < 0.25).count();
    assert!(worn > 0 && fresh > 0, "mixed condition: {worn} worn, {fresh} fresh");
    assert!(w2.containers.boxes.values().all(|r| valid_number(&r.number())));
    // Loading the same old save twice gives the same stock.
    let w3 = World::from_ron(&old).unwrap();
    let n = |w: &World| w.containers.boxes.values().map(|r| r.number()).collect::<Vec<_>>();
    assert_eq!(n(&w2), n(&w3));
}

/// Empties go back out of town: on an outgoing freight train as exports.
#[test]
fn exports_go_out_in_the_yards_empties() {
    let (mut w, yard, _) = town();
    let ids: Vec<cx::BoxId> = (0..4).map(|_| w.make_box(crate::rail::BoxKind::Dry, false, Where::Stored(yard), None, Event::ArrivedRail, yard)).collect();
    for id in &ids {
        w.containers.boxes[*id].built = w.clock.day() as i32 - 100;
        w.containers.boxes[*id].knocks = 0;
    }
    w.buildings[yard].rail_outbound = 40.0;
    assert!(w.spawn_train(TrainKind::Freight, &[yard]));
    let t = w.rail.trains.keys().next().unwrap();
    for _ in 0..400 {
        tick(&mut w, 1);
        if w.rail.trains.get(t).is_none_or(|t| t.boxes.iter().any(|b| matches!(b.lift, crate::rail::Lift::On(_)))) {
            break;
        }
    }
    let going = ids.iter().filter(|id| w.containers.boxes.get(**id).is_some_and(|r| r.at == Where::Train)).count();
    assert!(going >= 1, "an empty went on the train");
    assert!(w.containers.stats.sent_out_total >= 1);
}

// ── [r8] container lorries, bays, carriers, forwarders ─────────────────────

/// A young box at `at`, full of `goods` (or empty).
fn young_box(w: &mut World, at: BuildingId, forty: bool, goods: Option<cx::Goods>) -> cx::BoxId {
    let id = w.make_box(crate::rail::BoxKind::Dry, forty, Where::Stored(at), goods, Event::ArrivedRail, at);
    w.containers.boxes[id].built = w.clock.day() as i32 - 200;
    w.containers.boxes[id].knocks = 0;
    id
}

/// [r8] (Harvey flag 4) A container lorry pulls into a bay at the yard,
/// the carrier lifts the very box onto its forty-foot chassis, it delivers,
/// drives back with the empty, has it lifted off and stands in its bay
/// waiting for a box: on the road's books the whole time, never removed
/// while in town.
#[test]
fn a_lorry_waits_in_a_bay_and_never_vanishes_in_town() {
    let (mut w, yard, buyer) = town();
    let id = young_box(&mut w, yard, true, Some(cx::Goods { resource: Resource::Parts, buyer, amount: 0.0 }));
    w.note_inbound(buyer, Resource::Parts, 20.0);
    w.dispatch_truck(super::market::TruckOrigin::Building(yard), Cargo { from: yard, to: buyer, resource: Resource::Parts, amount: 20.0, price: 0.0 }, 6);
    let mut car = None;
    let mut lifted = false;
    let mut in_bay = false;
    for _ in 0..120 {
        tick(&mut w, 1);
        match w.containers.boxes[id].at {
            Where::Lifting(d) if d == yard => lifted = true,
            Where::Truck(c) => {
                car = Some(c);
                break;
            }
            _ => {}
        }
        if let Some((c, _)) = w.containers.lorries.iter().find(|(_, j)| j.depot == yard) {
            if w.lorry_bay(*c).is_some() {
                in_bay = true;
                let st = w.lorry_status(*c).unwrap();
                assert!(st.contains("parked at") || st.contains("in a bay at"), "{st}");
                assert!(w.traffic.cars[*c].bay, "drawn in the bay");
            }
        }
    }
    let car = car.expect("the box went onto a lorry");
    assert!(lifted && in_bay, "it stood in a bay and the carrier lifted the box: lifted {lifted} bay {in_bay}");
    // The chassis is the box's length.
    assert_eq!(crate::vehicle::chassis_of(w.traffic.cars[car].model.boxed), Some(true), "a forty-foot chassis under a forty-foot box");
    assert!(w.containers.boxes[id].forty);
    let mut delivered = false;
    let mut home = false;
    for _ in 0..4000 {
        tick(&mut w, 1);
        assert!(w.traffic.cars.contains_key(car), "the lorry is never taken off the road in town: {:?}", w.lorry_note(car));
        delivered |= w.containers.boxes[id].log.iter().any(|s| s.1 == Event::Delivered);
        if delivered && w.containers.boxes[id].at == Where::Stored(yard) && w.lorry_bay(car).is_some() && !w.containers.trucks.contains_key(&car) {
            home = true;
            break;
        }
    }
    assert!(delivered && home, "delivered, and back in a bay with the empty set down: {:?} {:?}", events(&w, id), w.lorry_status(car));
    assert_eq!(crate::vehicle::chassis_of(w.traffic.cars[car].model.boxed), Some(true), "an empty chassis is still a chassis");
    assert!(w.lorry_status(car).unwrap().contains("waiting for a box"), "{:?}", w.lorry_status(car));
}

/// [r8] A lorry that leaves the map says where it went.
#[test]
fn a_lorry_taken_off_the_road_leaves_a_note() {
    let (mut w, yard, _) = town();
    let car = w.spawn_box_lorry(yard, yard, yard).expect("a lorry at the gate");
    w.note_lorry(car, "left town by road".to_string());
    w.traffic.remove_car(car);
    assert_eq!(w.lorry_note(car), Some("left town by road"));
    assert!(!w.containers.lorries.contains_key(&car));
}

/// [r8] Every empty gets a use by its condition: past the depot's share of
/// room the worst go first (sent back out by their lines), sound ones stay
/// for the exports, and those fit only for scrap, with no scrap yard to
/// take them, are sold for scrap out of town after a wait instead of
/// piling up for ever.
#[test]
fn empties_are_used_by_condition_and_the_surplus_is_capped() {
    let (mut w, yard, _) = town();
    let cap = w.box_capacity(yard, DepotKind::Rail);
    let limit = ((cap as f32 * cx::SURPLUS_SHARE) as usize).min(cx::SURPLUS_MAX);
    let day = w.clock.day() as i32;
    let mut scrap = Vec::new();
    let mut worn = Vec::new();
    let mut sound = Vec::new();
    for k in 0..(limit + 20) {
        let id = young_box(&mut w, yard, k % 2 == 0, None);
        let r = &mut w.containers.boxes[id];
        // (Come in today, bar the scrap: no line has repositioned them yet.)
        r.since = 4 * crate::clock::MINUTES_PER_DAY;
        if k < 6 {
            r.since = 0;
            r.built = day - 365 * 23;
            scrap.push(id);
        } else if k < 22 {
            r.built = day - 365 * 13;
            worn.push(id);
        } else {
            sound.push(id);
        }
    }
    w.clock.tick += 4 * crate::clock::MINUTES_PER_DAY;
    let today = w.clock.day() as i64;
    w.daily_containers();
    // ([r11-box-stock] counted as each leaves town: gone, or on its way)
    let going = scrap.iter().filter(|id| w.containers.boxes.contains_key(**id)).count();
    assert_eq!(w.containers.stats.sold_for_scrap_total as usize + going, scrap.len(), "no scrap yard: sold for scrap out of town");
    for id in &scrap {
        let gone_or_going = w.containers.boxes.get(*id).is_none_or(|r| r.bound_out);
        assert!(gone_or_going, "a scrap box is on its way out");
    }
    let staying: Vec<cx::BoxId> = w.containers.boxes.iter().filter(|(_, r)| r.at == Where::Stored(yard) && !r.bound_out).map(|(id, _)| id).collect();
    assert!(staying.len() <= limit, "the surplus is capped: {} over a limit of {limit}", staying.len());
    // The worst went: every box going out is at least as worn as any kept.
    let kept_worst = staying.iter().map(|id| w.containers.boxes[*id].wear(today)).fold(0.0f32, f32::max);
    let out_best = w.containers.boxes.values().filter(|r| r.bound_out && !r.for_scrap(today)).map(|r| r.wear(today)).fold(1.0f32, f32::min);
    assert!(out_best + 1e-4 >= kept_worst || w.containers.boxes.values().filter(|r| r.bound_out && !r.for_scrap(today)).count() == 0, "worst first: out {out_best} kept {kept_worst}");
    assert!(sound.iter().filter(|id| staying.contains(id)).count() >= sound.len() / 2, "the sound ones are kept for the exports");
    // A freight train calling takes the ones going out first.
    let out_ids = w.empties_to_send(yard, 3);
    assert!(out_ids.iter().all(|id| w.containers.boxes[*id].bound_out), "the empties going out go first");
}

/// [r8] A freight forwarder: two firms' exports, each charged a fee, go
/// round in one leased box (sound, from the yard) on one lorry; the box is
/// set down at the freight yard full of both firms' goods and leaves town
/// on the next freight train, counted as exports.
#[test]
fn a_forwarder_fills_one_box_from_several_firms_and_charges_each() {
    let (mut w, yard, _) = town();
    let lot = free_lot(&w, LandUse::Industrial);
    let fw = w.open_business(lot, BusinessKind::FreightForwarder, true).expect("a forwarder");
    let lot = free_lot(&w, LandUse::Industrial);
    let f1 = w.open_business(lot, BusinessKind::Fab, true).expect("a firm");
    let lot = free_lot(&w, LandUse::Industrial);
    let f2 = w.open_business(lot, BusinessKind::Assembly, true).expect("another firm");
    let empty = young_box(&mut w, yard, true, None);
    let m1 = w.buildings[f1].business.as_ref().unwrap().money;
    let took = w.forwarders_take_exports(&[(f1, Resource::Parts, 24.0, 900.0), (f2, Resource::Electronics, 20.0, 800.0)]);
    assert!((took - 44.0).abs() < 0.01, "the forwarder takes both loads");
    assert!(w.buildings[f1].business.as_ref().unwrap().money < m1, "the firm pays the fee");
    let fees = w.containers.forwarders[&fw].fees_total;
    assert!(fees > 2.0 * cx::FORWARDER_FEE - 0.01, "a fee per load: {fees}");
    w.forwarder_rounds();
    assert_eq!(w.containers.boxes[empty].owner, Owner::Forwarder(fw), "it leased the sound empty at the yard");
    let mut done = false;
    for _ in 0..5000 {
        tick(&mut w, 1);
        let r = &w.containers.boxes[empty];
        if r.at == Where::Stored(yard) && r.export.len() == 2 {
            done = true;
            break;
        }
    }
    let r = &w.containers.boxes[empty];
    assert!(done, "full of both firms' goods, back at the yard: {:?} at {:?}", events(&w, empty), r.at);
    let firms: Vec<BuildingId> = r.export.iter().map(|l| l.firm).collect();
    assert!(firms.contains(&f1) && firms.contains(&f2));
    let ev = events(&w, empty);
    assert!(ev.contains(&Event::Leased) && ev.contains(&Event::CraneOn) && ev.contains(&Event::CraneOff) && ev.iter().filter(|e| **e == Event::Packed).count() == 2, "{ev:?}");
    let card = w.forwarder_card(fw).unwrap();
    assert_eq!(card.3.loads_total, 2);
    assert_eq!(card.3.firms.len(), 2);
    assert!(w.buildings[yard].rail_outbound > 0.0, "a train is due to take it");
    // Out on the next freight train.
    assert!(w.spawn_train(TrainKind::Freight, &[yard]));
    for _ in 0..400 {
        tick(&mut w, 1);
        if w.containers.boxes.get(empty).is_none_or(|r| r.at == Where::Train) {
            break;
        }
    }
    assert!(w.containers.boxes.get(empty).is_none_or(|r| r.at == Where::Train), "on the train: {:?}", w.containers.boxes.get(empty).map(|r| r.at));
    assert!(w.containers.stats.exported_total >= 1);
    assert_eq!(w.containers.forwarders[&fw].boxes_out, 1);
}

/// [r8] Lorries in bays, the carrier's work, forwarders' books and the
/// boxes aboard ships come back from a save as they were.
#[test]
fn bays_jobs_and_books_survive_a_save() {
    let (mut w, yard, buyer) = town();
    young_box(&mut w, yard, false, Some(cx::Goods { resource: Resource::Parts, buyer, amount: 0.0 }));
    w.dispatch_truck(super::market::TruckOrigin::Building(yard), Cargo { from: yard, to: buyer, resource: Resource::Parts, amount: 10.0, price: 0.0 }, 6);
    tick(&mut w, 2);
    assert!(!w.containers.depots.is_empty() && !w.containers.lorries.is_empty());
    w.containers.forwarders.insert(buyer, cx::Forwarder { loads_total: 3, fees_total: 99.0, ..Default::default() });
    let ron = w.to_ron().unwrap();
    let w2 = World::from_ron(&ron).unwrap();
    assert_eq!(w.containers.depots, w2.containers.depots);
    assert_eq!(w.containers.lorries.keys().collect::<Vec<_>>(), w2.containers.lorries.keys().collect::<Vec<_>>());
    assert_eq!(w.containers.forwarders, w2.containers.forwarders);
    let bays: Vec<bool> = w.traffic.cars.values().map(|c| c.bay).collect();
    let bays2: Vec<bool> = w2.traffic.cars.values().map(|c| c.bay).collect();
    assert_eq!(bays, bays2);
    let at: Vec<Where> = w.containers.boxes.values().map(|r| r.at).collect();
    let at2: Vec<Where> = w2.containers.boxes.values().map(|r| r.at).collect();
    assert_eq!(at, at2);
}

// ── [r10-port-boxes] places in the stacks ──────────────────────────────────

/// [r10-port-boxes] (Harvey flag, day 80: "boxes are stacked on top of each
/// other again") A box lifted out onto a lorry's chassis gives up its place
/// in the stack; boxes coming in meanwhile take the free places, and the
/// box coming back off the lorry takes a place of its own: no two boxes
/// ever share one, at any tick of the round trip.
#[test]
fn a_box_back_off_a_lorry_takes_a_free_place_in_the_stack() {
    let (mut w, yard, buyer) = town();
    for k in 0..5 {
        young_box(&mut w, yard, k % 3 != 1, None);
    }
    let id = young_box(&mut w, yard, true, Some(cx::Goods { resource: Resource::Parts, buyer, amount: 0.0 }));
    let first = w.containers.boxes[id].slot.expect("a place in the stack");
    assert_eq!(w.containers.slot_clashes(), 0);
    w.note_inbound(buyer, Resource::Parts, 20.0);
    w.dispatch_truck(super::market::TruckOrigin::Building(yard), Cargo { from: yard, to: buyer, resource: Resource::Parts, amount: 20.0, price: 0.0 }, 6);
    let mut away = false;
    let mut back = false;
    let mut newcomers = Vec::new();
    for _ in 0..5000 {
        tick(&mut w, 1);
        assert_eq!(w.containers.slot_clashes(), 0, "two boxes in one place at tick {}", w.clock.tick);
        let r = &w.containers.boxes[id];
        if !away && matches!(r.at, Where::Truck(_)) {
            away = true;
            assert_eq!(r.slot.filter(|_| r.at.stack().is_some()), None, "off the stack, out of its place");
            // Two boxes landed while it is out: the first takes its place.
            newcomers.push(young_box(&mut w, yard, true, None));
            newcomers.push(young_box(&mut w, yard, false, None));
            assert_eq!(w.containers.boxes[newcomers[0]].slot, Some(first), "the free place is filled");
        }
        if away && w.containers.boxes[id].at == Where::Stored(yard) {
            back = true;
            break;
        }
    }
    assert!(away && back, "out on a lorry and back: {:?}", events(&w, id));
    let s = w.containers.boxes[id].slot.expect("a place again");
    assert_ne!(s, first, "not on top of the box that took its old place");
    assert_eq!(s % 2, 0, "a forty-footer on an even cell");
    assert_eq!(w.containers.slot_clashes(), 0);
}

/// [r10-port-boxes] Every way into a stack seats the box: twenty- and
/// forty-footers mixed, boxes lifted out and set down at random, a scrap
/// pile, an empty sent to storage; never two in one place, and a stack
/// fills from its first places.
#[test]
fn every_box_in_a_stack_has_a_place_of_its_own() {
    let (mut w, yard, _) = town();
    let mut ids = Vec::new();
    for k in 0..40u64 {
        let h = crate::hash::mix(k, 0x51_07);
        ids.push(young_box(&mut w, yard, h & 1 == 0, None));
        if h % 5 == 0 {
            // One out (onto a lorry, a site, the crane) and back.
            let out = ids[(h as usize / 7) % ids.len()];
            w.box_to(out, Where::Truck(crate::traffic::CarId::default()), Event::CraneOn, yard);
            assert_eq!(w.containers.slot_clashes(), 0);
            if h % 3 == 0 {
                w.box_to(out, Where::Stored(yard), Event::CraneOff, yard);
            }
        }
        assert_eq!(w.containers.slot_clashes(), 0, "after box {k}");
    }
    // A box set down again where it stands keeps its place.
    let keep = *ids.iter().find(|id| w.containers.boxes[**id].at == Where::Stored(yard)).unwrap();
    let s = w.containers.boxes[keep].slot;
    w.box_to(keep, Where::Stored(yard), Event::Stored, yard);
    assert_eq!(w.containers.boxes[keep].slot, s);
    // Packed from the front: the cells taken are about as many as the TEU.
    let teu: u32 = w.containers.boxes.values().filter(|r| r.at == Where::Stored(yard)).map(|r| if r.forty { 2 } else { 1 }).sum();
    let top = w.containers.boxes.values().filter(|r| r.at == Where::Stored(yard)).map(|r| r.slot.unwrap() as u32 + if r.forty { 2 } else { 1 }).max().unwrap();
    assert!(top <= teu + teu / 4 + 2, "no far-off places: top {top} for {teu} TEU");
}

/// [r10-port-boxes] An older save (no places yet), or one where two boxes
/// share a place, loads with every stored box in a place of its own: the
/// first come keeps its place, the rest go to the first free ones, the
/// same way every time it is loaded.
#[test]
fn boxes_sharing_a_place_are_reseated_on_load() {
    let (mut w, yard, _) = town();
    let mut ids = Vec::new();
    for k in 0..8 {
        ids.push(young_box(&mut w, yard, k % 3 != 2, None));
        w.containers.boxes[*ids.last().unwrap()].since = 100 + k as u64;
    }
    // Everyone in the first place (the bug), and two with none at all (an
    // older save).
    for id in &ids {
        w.containers.boxes[*id].slot = Some(0);
    }
    w.containers.boxes[ids[6]].slot = None;
    w.containers.boxes[ids[7]].slot = None;
    assert!(w.containers.slot_clashes() > 0);
    let ron = w.to_ron().unwrap();
    let a = World::from_ron(&ron).unwrap();
    let b = World::from_ron(&ron).unwrap();
    assert_eq!(a.containers.slot_clashes(), 0, "re-seated on load");
    assert!(ids.iter().all(|id| a.containers.boxes[*id].slot.is_some()));
    assert_eq!(a.containers.boxes[ids[0]].slot, Some(0), "the first come keeps its place");
    let places = |w: &World| ids.iter().map(|id| w.containers.boxes[*id].slot).collect::<Vec<_>>();
    assert_eq!(places(&a), places(&b), "the same every load");
    // A save without the field at all: every stored box seated.
    let old = ron.replace("slot:Some(0),", "").replace("slot: Some(0),", "");
    let c = World::from_ron(&old).unwrap();
    assert_eq!(c.containers.slot_clashes(), 0);
    assert!(ids.iter().all(|id| c.containers.boxes[*id].slot.is_some()));
}

// ── [r11-box-stock] the depots' stock kept within their room ────────────────

/// A container yard business on the industrial land across the street.
fn storage_yard(w: &mut World) -> BuildingId {
    let lot = free_lot(w, LandUse::Industrial);
    let y = w.open_business(lot, BusinessKind::ContainerYard, true).expect("a container yard");
    assert_eq!(w.box_depot(y), Some(DepotKind::Yard));
    y
}

/// Every box ever made is still in town, or counted out of it by why:
/// none vanishes without a record. (A box put on a train going out is
/// counted as it goes on: not again while the train is still in town.)
fn accounted(w: &World) -> u64 {
    let s = &w.containers.stats;
    let on_the_way_out = w.containers.boxes.values().filter(|r| r.at == Where::Train && r.log.last().is_some_and(|l| matches!(l.1, Event::SentOut | Event::Exported))).count() as u64;
    w.containers.boxes.len() as u64 - on_the_way_out + s.sent_out_total as u64 + s.sold_out_of_town_total as u64 + s.exported_total as u64 + s.scrapped_total as u64 + s.sold_for_scrap_total as u64
}

/// Where the boxes are and what last happened to them (for a failed count).
fn census(w: &World) -> String {
    let mut m: BTreeMap<String, u32> = BTreeMap::new();
    for r in w.containers.boxes.values() {
        let at = format!("{:?}", r.at);
        let at = at.split('(').next().unwrap_or("").to_string();
        *m.entry(format!("{at}/{:?}", r.log.last().map(|l| l.1))).or_default() += 1;
    }
    format!("{m:?} {:?}", w.containers.stats)
}

/// [r11-box-stock] A terminal over its fill trucks its lines' overflow to a
/// container yard, which keeps it for a fee a day; the terminal stays
/// within its room, the boxes' stories say where they are kept, the yard's
/// books show the fees, and the terminal calls its boxes back once it runs
/// low.
#[test]
fn a_full_terminal_keeps_its_overflow_at_a_container_yard_for_a_fee() {
    let (mut w, yard, _) = town();
    let store = storage_yard(&mut w);
    let room = w.box_room(yard, DepotKind::Rail);
    let fill = (room as f32 * cx::TERMINAL_FILL) as usize;
    for k in 0..room {
        young_box(&mut w, yard, k % 3 == 0, None);
    }
    let made = w.containers.stats.made;
    let before = accounted(&w);
    assert_eq!(before, made);
    tick(&mut w, 2 * 1440);
    assert!(w.depot_stock(yard) <= fill + 1, "the terminal is back under its fill: {} of {room} (fill {fill})", w.depot_stock(yard));
    let kept: Vec<cx::BoxId> = w.containers.boxes.iter().filter(|(_, r)| r.kept_for == Some(yard) && r.at == Where::Stored(store)).map(|(id, _)| id).collect();
    assert!(!kept.is_empty(), "its overflow is kept at the container yard: {:?}", w.containers.stats);
    assert_eq!(w.kept_away(yard), w.containers.boxes.values().filter(|r| r.kept_for == Some(yard)).count());
    let story = w.box_story(kept[0]).unwrap();
    assert!(story.now.starts_with("kept at") && story.now.contains("overflow of"), "{}", story.now);
    let card = w.storage_card(store).expect("the yard's storage books");
    assert!(card.kept >= kept.len() && card.book.fees_total >= cx::STORAGE_FEE, "the lines pay the yard: {card:?}");
    assert!(w.containers.stats.storage_fees_total > 0.0);
    // Nothing vanished.
    assert_eq!(accounted(&w), w.containers.stats.made, "every box is in town or counted out of it: {}", census(&w));
    // The terminal runs low (the boxes there taken away): it calls its boxes back.
    let there: Vec<cx::BoxId> = w.containers.boxes.iter().filter(|(_, r)| r.at == Where::Stored(yard)).map(|(id, _)| id).collect();
    for id in there {
        w.containers.boxes.remove(id);
        w.containers.stats.sent_out_total += 1;
    }
    tick(&mut w, 1440);
    assert!(w.containers.stats.called_back_total >= 1, "called back: {:?}", w.containers.stats);
    assert!(w.containers.boxes.values().any(|r| r.at == Where::Stored(yard) && r.log.iter().any(|s| s.2 == store)), "a box from the yard is back at the terminal");
    assert_eq!(accounted(&w), w.containers.stats.made);
}

/// [r11-box-stock] (Harvey's day-80 save: 1375 boxes at a port with room
/// for 60) A terminal far over its room with nowhere in town to keep the
/// rest works the backlog off over a few days, lorries carrying it out of
/// town by road (every box counted as sent out), and the boxes no yard
/// could keep make the pressure for a container storage yard.
#[test]
fn an_overstocked_terminal_drains_by_road_and_asks_for_a_storage_yard() {
    let (mut w, yard, _) = town();
    let room = w.box_room(yard, DepotKind::Rail);
    for k in 0..room * 6 {
        young_box(&mut w, yard, k % 2 == 0, None);
    }
    let mut by_day = Vec::new();
    for _ in 0..6 {
        tick(&mut w, 1440);
        by_day.push(w.depot_stock(yard));
        assert_eq!(accounted(&w), w.containers.stats.made, "every box is in town or counted out of it: {}", census(&w));
    }
    assert!(by_day.windows(2).all(|p| p[1] <= p[0] + 2), "the stock falls day by day: {by_day:?}");
    assert!(*by_day.last().unwrap() <= room, "within its room after a few days: {by_day:?} (room {room})");
    assert!(w.containers.stats.cleared_by_road_total as usize >= room * 4, "out of town by road: {:?}", w.containers.stats);
    // The pressure: the boxes no yard could keep.
    w.daily_pressure();
    assert!(w.pressure.signals.box_overflow > 0.0 || w.containers.stats.overflow_yday > 0);
    let s = crate::pressure::Signals { box_overflow: 120.0, box_terminals: 1.0, ..Default::default() };
    let p = crate::pressure::assess(&s, &w.pressure_supply());
    let r = p.iter().find(|r| r.kind == crate::pressure::PressureKind::ContainerStorage).unwrap();
    assert!(r.wants_more(), "the overflow wants a container yard: {r:?}");
    assert_eq!(r.kind.business(), Some(BusinessKind::ContainerYard));
}

/// [r11-box-stock] Over many days of boxes landing at a terminal (more
/// than leave by train), with a container yard in town: the terminal's
/// stock stays within its room every midnight, the yard's within its own,
/// and no box vanishes without a record.
#[test]
fn stock_stays_within_room_over_many_days_of_landings() {
    let (mut w, yard, buyer) = town();
    let store = storage_yard(&mut w);
    let room = w.box_room(yard, DepotKind::Rail);
    let store_room = w.box_room(store, DepotKind::Yard);
    let mut worst = 0usize;
    for day in 0..10 {
        for h in 0..24 {
            // Two boxes land an hour, one of them full for the buyer.
            young_box(&mut w, yard, h % 2 == 0, None);
            if h % 3 == 0 {
                young_box(&mut w, yard, false, Some(cx::Goods { resource: Resource::Parts, buyer, amount: 0.0 }));
            }
            tick(&mut w, 60);
        }
        let stock = w.depot_stock(yard);
        worst = worst.max(stock);
        assert!(stock <= room, "day {day}: the terminal holds {stock}, room {room}");
        assert!(w.depot_stock(store) <= store_room, "day {day}: the yard holds {}, room {store_room}", w.depot_stock(store));
        assert_eq!(accounted(&w), w.containers.stats.made, "day {day}: every box is in town or counted out of it");
    }
    assert!(w.containers.stats.to_storage_total > 0, "some went to the yard to be kept");
    assert!(worst > 0);
}

/// [r11-box-stock] The pressure for a container storage yard, with no
/// vacant lot that suits: a shut-down industrial business near the
/// terminals is turned into one.
#[test]
fn a_shut_industrial_business_becomes_a_storage_yard() {
    let (mut w, _yard, _) = town();
    let lot = free_lot(&w, LandUse::Industrial);
    let b = w.open_business(lot, BusinessKind::Slaughterhouse, true).expect("a works");
    w.buildings[b].floors = w.buildings[b].floors.min(2);
    w.buildings[b].plot.form = None;
    w.buildings[b].business.as_mut().unwrap().closed = true;
    assert!(w.box_depot(b).is_none());
    let y = w.convert_to_storage_yard("boxes with nowhere to go").expect("turned into a yard");
    assert_eq!(y, b);
    assert_eq!(w.box_depot(b), Some(DepotKind::Yard), "a working container yard now");
    assert!(w.buildings[b].business.as_ref().unwrap().origin.as_deref().unwrap_or("").contains("container yard"));
}
