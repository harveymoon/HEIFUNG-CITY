//! [r12-hunger] Hunger and food: lost orders written off so grocers order
//! again, hub loads that never come sent by road, and hunger where people
//! live asking for grocers and places cooking meals there.

use super::*;
use crate::cargo_hub::{HubLoad, HubState, Via};
use crate::market::{Business, BusinessKind, Resource};
use crate::mind::IdleMind;
use super::food_need::{FoodNeed, FOOD_PULL, NEED_CELL};

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

fn lot_near(w: &World, p: Vec2, lu: LandUse) -> LotId {
    w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == lu).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|(id, _)| id).expect("a vacant lot")
}

/// Nothing on its way anywhere and nothing booked: a test's own loads only.
fn clear_freight(w: &mut World) {
    w.freight_queue.clear();
    let on_road: Vec<CarId> = w.traffic.cars.iter().filter(|(_, c)| c.cargo.is_some()).map(|(id, _)| id).collect();
    for c in on_road {
        w.traffic.remove_car(c);
    }
    for b in w.buildings.values_mut() {
        if let Some(z) = b.business.as_mut() {
            z.inbound.clear();
        }
    }
}

/// A street of homes with two grocers across it and a vacant commercial
/// lot or more left, and a few households moved in.
fn grocer_street() -> (World, BuildingId, BuildingId) {
    let mut w = World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.add_zone(square(-390.0, 10.0, 390.0), LandUse::Commercial);
    for x in [40.0, 100.0, 160.0] {
        let lot = lot_near(&w, Vec2::new(x, 20.0), LandUse::Residential);
        w.build_on(lot);
    }
    let mut shops = Vec::new();
    for x in [-60.0, -340.0] {
        let lot = lot_near(&w, Vec2::new(x, 20.0), LandUse::Commercial);
        w.build_on(lot);
        shops.push(lot);
    }
    let sites: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in sites {
        w.finish_construction(id);
    }
    let ids: Vec<BuildingId> = shops.iter().map(|l| w.lots[*l].building.unwrap()).collect();
    for &b in &ids {
        let jobs = w.buildings[b].jobs;
        w.buildings[b].business = Some(Business::new(BusinessKind::Grocery, jobs));
    }
    let mut minds = IdleMind;
    for _ in 0..900 {
        if w.households.len() >= 3 {
            break;
        }
        w.tick(&mut minds);
    }
    assert!(w.households.len() >= 2, "households moved in");
    (w, ids[0], ids[1])
}

/// A grocer whose produce order was lost on the way (booked, but in no
/// lorry, queue, hub or depot) has the booking written off at the second
/// audit and orders produce again; one with its load really on the way
/// keeps its booking.
#[test]
fn a_lost_order_is_written_off_and_the_grocer_orders_again() {
    let (mut w, lost, real) = grocer_street();
    clear_freight(&mut w);
    for g in [lost, real] {
        w.buildings[g].business.as_mut().unwrap().stock.insert(Resource::Produce, 0.0);
    }
    w.buildings[lost].business.as_mut().unwrap().inbound.insert(Resource::Produce, 50.0);
    w.note_inbound(real, Resource::Produce, 40.0);
    let q = super::delivery_hours::QueuedTruck { due: u64::MAX, from: None, cargo: Cargo { from: BuildingId::default(), to: real, resource: Resource::Produce, amount: 40.0, price: 0.0 }, color: 0 };
    w.freight_queue.push(q);
    assert_eq!(w.audit_inbound(), 0.0, "the first audit only takes note");
    assert!(w.inbound_unanswered.iter().any(|(b, r, a)| *b == lost && *r == Resource::Produce && (*a - 50.0).abs() < 0.01));
    let off = w.audit_inbound();
    assert!((off - 50.0).abs() < 0.01, "the second writes the lost order off, got {off}");
    assert_eq!(w.economy.market.food.today.written_off, off, "counted");
    assert_eq!(w.buildings[lost].business.as_ref().unwrap().inbound_of(Resource::Produce), 0.0);
    assert_eq!(w.buildings[real].business.as_ref().unwrap().inbound_of(Resource::Produce), 40.0, "a load on its way keeps its booking");
    // The next restock orders produce for the grocer again.
    w.hourly_economy();
    let z = w.buildings[lost].business.as_ref().unwrap();
    assert!(z.inbound_of(Resource::Produce) + z.stock_of(Resource::Produce) > 0.0, "the grocer orders produce again");
}

/// A booking unanswered at one audit only (its load caught between two
/// places at that moment) is never written off.
#[test]
fn a_booking_unanswered_once_is_kept() {
    let (mut w, g, _) = grocer_street();
    clear_freight(&mut w);
    w.note_inbound(g, Resource::Dairy, 30.0);
    let q = super::delivery_hours::QueuedTruck { due: u64::MAX, from: None, cargo: Cargo { from: BuildingId::default(), to: g, resource: Resource::Dairy, amount: 30.0, price: 0.0 }, color: 0 };
    w.freight_queue.push(q);
    w.audit_inbound();
    w.freight_queue.clear();
    assert_eq!(w.audit_inbound(), 0.0, "unanswered once: noted");
    w.freight_queue.push(q);
    assert_eq!(w.audit_inbound(), 0.0, "answered again: nothing written off");
    assert_eq!(w.buildings[g].business.as_ref().unwrap().inbound_of(Resource::Dairy), 30.0);
    assert!(w.inbound_unanswered.is_empty());
}

/// A hub's sea load that never came (expected for two days, on no ship)
/// comes by road to its buyer; a fresh one stays expected.
#[test]
fn a_hub_load_that_never_comes_goes_by_road() {
    let (mut w, buyer, hub) = grocer_street();
    w.buildings[buyer].business.as_mut().unwrap().inbound.clear();
    let before = w.buildings[buyer].business.as_ref().unwrap().stock_of(Resource::Flour);
    w.note_inbound(buyer, Resource::Flour, 12.0);
    w.note_inbound(buyer, Resource::Flour, 7.0);
    w.clock.tick += 3 * 1440;
    let now = w.clock.tick;
    let old = HubLoad { buyer, resource: Resource::Flour, amount: 12.0, price: 10.0, since: now - 49 * 60, via: Via::Sea };
    let fresh = HubLoad { since: now - 60, amount: 7.0, ..old };
    w.cargo_hubs.hubs.insert(hub, HubState { inbound: vec![old, fresh], ..Default::default() });
    assert_eq!(w.give_up_hub_loads(), 1);
    assert_eq!(w.cargo_hubs.hubs[&hub].inbound, vec![fresh], "the fresh load is still expected");
    // By road: a lorry on its way to the buyer with it, or (no road in
    // from the edge) handed over at once.
    let z = w.buildings[buyer].business.as_ref().unwrap();
    let lorry = w.traffic.cars.values().any(|c| c.cargo.is_some_and(|c| c.to == buyer && c.resource == Resource::Flour && c.amount == 12.0));
    let handed = z.stock_of(Resource::Flour) >= before + 12.0 && z.inbound_of(Resource::Flour) == 7.0;
    assert!(lorry || handed, "the late load comes by road");
}

/// The hungry are counted where they live: their home's cell and its
/// share of the town; the fed are not.
#[test]
fn the_hungry_are_counted_where_they_live() {
    let (mut w, _, _) = grocer_street();
    let people: Vec<CitizenId> = w.citizens.keys().collect();
    assert!(people.len() >= 2);
    for c in w.citizens.values_mut() {
        c.satiety = 0.9;
        c.hungry_days = 0;
    }
    let hungry = people[0];
    w.citizens[hungry].satiety = 0.1;
    w.daily_food_need();
    assert_eq!(w.food_need.hungry, 1);
    assert!((w.food_need.share - 1.0 / people.len() as f32).abs() < 1e-4);
    let home = w.citizens[hungry].home.expect("housed");
    let at = w.lots[w.buildings[home].lot].center;
    assert!(w.food_need.hungry_near(at) > 0.0, "counted at home");
    assert!(w.food_need.hungry_near(at + Vec2::new(10.0 * NEED_CELL, 0.0)) == 0.0, "not across town");
    // Fed again: the reading fades.
    w.citizens[hungry].satiety = 0.9;
    for _ in 0..8 {
        w.daily_food_need();
    }
    assert!(w.food_need.cells.is_empty(), "faded away: {:?}", w.food_need.cells);
}

/// Hunger where people live makes a new commercial building there a
/// grocer or a place cooking meals, and draws builders to the lot; with
/// nobody hungry it does neither. A hungry town wants a grocer even when
/// the head count says it has enough.
#[test]
fn hunger_asks_for_grocers_and_places_to_eat_where_people_are_hungry() {
    let (mut w, _, _) = grocer_street();
    let lot = lot_near(&w, Vec2::new(-200.0, 20.0), LandUse::Commercial);
    assert_eq!(w.hunger_pull_at(lot), 0.0);
    assert_eq!(w.hunger_business(lot, LandUse::Commercial, 6), None, "nobody hungry: no ask");
    // The town's head count is covered: no grocer by it alone.
    w.economy.homes_building = 0;
    let jobs_now: f32 = w.buildings.values().filter(|b| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Grocery)).map(|b| b.jobs as f32).sum();
    assert!(jobs_now >= w.grocery_jobs_needed(), "two grocers cover this street");
    assert!(!w.kinds_viable(&[BusinessKind::Grocery], Some(6)).contains(&BusinessKind::Grocery), "fed: no more grocers");
    // Forty hungry living round the lot.
    let cell = FoodNeed::cell_of(w.lots[lot].center);
    let mut today = BTreeMap::new();
    today.insert(cell, 80.0);
    w.food_need.blend(&today, 80, 1000);
    assert!(w.food_need.town_hungry());
    assert!(w.hunger_pull_at(lot) >= FOOD_PULL, "pull {}", w.hunger_pull_at(lot));
    assert!(w.kinds_viable(&[BusinessKind::Grocery], Some(6)).contains(&BusinessKind::Grocery), "hungry: a grocer is worth opening");
    let k = w.hunger_business(lot, LandUse::Commercial, 6).expect("a food business");
    assert!(super::food_need::FOOD_KINDS.contains(&k), "{k:?}");
    // Built on, it opens as one, and counts as opened for hunger.
    let id = w.build_on_as(lot, None).expect("built");
    let kind = w.buildings[id].business.as_ref().map(|z| z.kind);
    assert!(kind.is_some_and(|k| super::food_need::FOOD_KINDS.contains(&k)), "opened as {kind:?}");
    assert_eq!(w.food_need.opened_total, 1);
    // Not on industrial land, and not far from the hungry.
    assert_eq!(w.hunger_business(lot, LandUse::Industrial, 6), None);
    let far = w.food_need.pull(w.lots[lot].center + Vec2::new(5.0 * NEED_CELL, 0.0));
    assert_eq!(far, 0.0);
}

/// A grocer with bare shelves where people are hungry buys groceries in
/// while the town's stock stands in another shop across town; with nobody
/// hungry round it, it waits for its own supplies.
#[test]
fn a_bare_grocer_where_people_are_hungry_buys_groceries_in() {
    let run = |hungry: bool| {
        let (mut w, bare, full) = grocer_street();
        for g in [bare, full] {
            let z = w.buildings[g].business.as_mut().unwrap();
            z.stock.clear();
            z.inbound.clear();
            z.money = 10_000.0;
        }
        // Inputs booked as on their way, so it cannot just order them.
        for r in [Resource::Produce, Resource::Flour, Resource::Meat, Resource::Dairy] {
            w.note_inbound(bare, r, 500.0);
        }
        w.buildings[full].business.as_mut().unwrap().add(Resource::Groceries, 5_000.0);
        if hungry {
            let mut today = BTreeMap::new();
            today.insert(FoodNeed::cell_of(w.lots[w.buildings[bare].lot].center), 60.0);
            w.food_need.blend(&today, 60, 600);
        }
        w.economy.market.food.today = Default::default();
        w.hourly_economy();
        (w.economy.market.food.today.imported, w.buildings[bare].business.as_ref().unwrap().inbound_of(Resource::Groceries))
    };
    let (imported, inbound) = run(true);
    assert!(imported > 0.0 && inbound > 0.0, "hungry round it: groceries bought in ({imported}, {inbound})");
    let (imported, _) = run(false);
    assert_eq!(imported, 0.0, "nobody hungry round it: the town's stock says enough");
}

/// In a town built up to the last lot, a shut shopfront where people are
/// hungry becomes a grocer; nothing changes while the town is fed, nor
/// where nobody round it is hungry.
#[test]
fn a_shut_shop_where_people_are_hungry_becomes_a_grocer() {
    let (mut w, shop, _) = grocer_street();
    let jobs = w.buildings[shop].jobs;
    let mut z = Business::new(BusinessKind::GadgetShop, jobs);
    z.closed = true;
    w.buildings[shop].business = Some(z);
    let c = FoodNeed::cell_of(w.lots[w.buildings[shop].lot].center);
    let hungry_at = |w: &mut World, cell: (i32, i32), hungry: u32| {
        w.food_need = FoodNeed::default();
        let mut today = BTreeMap::new();
        today.insert(cell, 60.0);
        w.food_need.blend(&today, hungry, 1000);
    };
    hungry_at(&mut w, c, 1);
    assert!(!w.food_need.town_hungry());
    assert_eq!(w.daily_food_retool(), 0, "a fed town turns nothing into grocers");
    hungry_at(&mut w, (c.0 + 5, c.1), 300);
    assert_eq!(w.daily_food_retool(), 0, "nobody hungry round the shop");
    hungry_at(&mut w, c, 300);
    assert_eq!(w.daily_food_retool(), 1, "hungry round it: it becomes a grocer");
    let z = w.buildings[shop].business.as_ref().unwrap();
    assert_eq!((z.kind, z.closed), (BusinessKind::Grocery, false));
    assert!(z.origin.as_deref().is_some_and(|o| o.contains("hungry")));
    assert_eq!(w.food_need.opened_total, 1);
}

/// A grocer still trading repairs its run-down building before it falls
/// derelict (a derelict shop takes no customers); one that cannot pay, or
/// has no staff, runs down.
#[test]
fn a_trading_shop_keeps_its_building_up() {
    let (mut w, shop, other) = grocer_street();
    let worker = w.citizens.keys().next().unwrap();
    for b in [shop, other] {
        w.buildings[b].workers = vec![worker];
        w.buildings[b].condition = 0.25;
    }
    assert!(!w.buildings[shop].takes_customers(), "derelict: no customers");
    w.buildings[shop].business.as_mut().unwrap().money = 1_000.0;
    w.buildings[other].business.as_mut().unwrap().money = 5.0;
    w.buildings[other].business.as_mut().unwrap().stock.clear();
    let jobs = w.buildings[shop].jobs.max(1) as f32;
    w.keep_up_shop(shop);
    w.keep_up_shop(other);
    assert!(w.buildings[shop].condition >= 0.8 && w.buildings[shop].takes_customers(), "repaired and open to customers");
    assert!((w.buildings[shop].business.as_ref().unwrap().money - (1_000.0 - 20.0 * jobs)).abs() < 0.01, "paid for it");
    assert_eq!(w.buildings[other].condition, 0.25, "nothing to pay with: it runs down");
    // Full shelves pay for it when the till is empty.
    w.buildings[other].business.as_mut().unwrap().add(Resource::Groceries, 500.0);
    w.keep_up_shop(other);
    assert!(w.buildings[other].condition >= 0.8, "its stock pays");

    assert_eq!(w.economy.market.food.today.shops_repaired, 2);
    // A tenant repairs the building its unit is in.
    let host = other;
    w.buildings[host].condition = 0.25;
    w.buildings[shop].unit_of = Some(host);
    w.buildings[shop].condition = 0.25;
    w.keep_up_shop(shop);
    assert!(w.buildings[host].condition >= 0.8, "the host is repaired by its tenant");
    w.buildings[shop].unit_of = None;
    // Unstaffed, it is left to run down.
    w.buildings[shop].condition = 0.25;
    w.buildings[shop].workers.clear();
    w.keep_up_shop(shop);
    assert_eq!(w.buildings[shop].condition, 0.25);
}
