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
    assert_eq!(r.at, Where::Stored(yard), "back in storage at the yard");
    let ev = events(&w, id);
    assert_eq!(ev[0], Event::ArrivedRail);
    assert!(ev.contains(&Event::OnLorry) && ev.contains(&Event::Stored), "{ev:?}");
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
    let id = w.make_box(crate::rail::BoxKind::Dry, false, Where::Stored(yard), Some(cx::Goods { resource: Resource::Parts, buyer }), Event::ArrivedRail, yard);
    w.containers.boxes[id].built = w.clock.day() as i32 - 365 * 3;
    w.containers.boxes[id].knocks = 0;
    w.note_inbound(buyer, Resource::Parts, 20.0);
    w.dispatch_truck(super::market::TruckOrigin::Building(yard), Cargo { from: yard, to: buyer, resource: Resource::Parts, amount: 20.0, price: 0.0 }, 6);
    let car = match w.containers.boxes[id].at {
        Where::Truck(c) => c,
        other => panic!("the box rides on the lorry: {other:?}"),
    };
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
    w.build_on(site_lot);
    let site = w.lots[site_lot].building.unwrap();
    let before = w.buildings[scrap].business.as_ref().unwrap().stock_of(Resource::Metal);
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
