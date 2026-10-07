//! Tests of cargo hubs: consolidated imports, local delivery, sidings.

use super::*;
use crate::cargo_hub as hub;
use crate::mind::IdleMind;

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

/// A street to the map edge with an office block at one end and
/// industrial land for the hub at the other; the buyer is an office (it
/// uses none of what it is sent, so deliveries can be counted).
fn town() -> (World, BuildingId, LotId) {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-600.0, 0.0), Vec2::new(600.0, 0.0)], 1, 1);
    w.add_zone(square(-400.0, 10.0, 120.0, 60.0), LandUse::Commercial);
    w.add_zone(square(100.0, 10.0, 300.0, 110.0), LandUse::Industrial);
    let shop = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Commercial && l.car_access()).map(|(id, _)| id).next().unwrap();
    w.build_on(shop);
    let b = w.lots[shop].building.unwrap();
    w.finish_construction(b);
    let jobs = w.buildings[b].jobs;
    w.buildings[b].business = Some(Business::new(BusinessKind::Office, jobs));
    let site = w.lots.iter().filter(|(_, l)| l.building.is_none() && l.usable() && l.land_use == LandUse::Industrial && l.car_access()).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, _)| id).unwrap();
    (w, b, site)
}

fn edge_semis(w: &World, buyer: BuildingId) -> usize {
    w.traffic.cars.values().filter(|c| c.cargo.is_some_and(|x| x.to == buyer && x.from == BuildingId::default())).count()
}

#[test]
fn without_a_hub_every_import_is_its_own_semi() {
    let (mut w, buyer, _) = town();
    let before = edge_semis(&w, buyer);
    for _ in 0..3 {
        w.import(buyer, Resource::Produce, 10.0, 20.0);
    }
    assert_eq!(edge_semis(&w, buyer) - before, 3);
}

#[test]
fn a_hub_brings_imports_in_one_semi_and_delivers_them_on() {
    let (mut w, buyer, site) = town();
    w.economy.treasury = 100_000.0;
    let h = w.build_cargo_hub(site).expect("hub placed");
    assert!(w.cargo_hub_works(h));
    assert!(w.fleet_capacity(h) >= 4, "the hub has bays for its lorries");
    let stock0 = w.buildings[buyer].business.as_ref().unwrap().stock_of(Resource::Produce);
    let before = edge_semis(&w, buyer);
    for _ in 0..3 {
        w.import(buyer, Resource::Produce, 10.0, 20.0);
    }
    assert_eq!(edge_semis(&w, buyer), before, "no semi straight to the buyer");
    assert_eq!(w.cargo_hubs.hubs[&h].waiting.len(), 3);
    // Cheaper by road through the hub.
    let mut minds = IdleMind;
    let mut trunks = 0;
    for _ in 0..600 {
        w.tick(&mut minds);
        trunks = trunks.max(w.cargo_hubs.stats.trunks_in_today);
        let s = w.buildings[buyer].business.as_ref().unwrap().stock_of(Resource::Produce);
        if s >= stock0 + 30.0 - 0.01 {
            break;
        }
    }
    assert_eq!(trunks, 1, "one semi for all three");
    assert_eq!(w.cargo_hubs.stats.long_haul_saved_today, 2);
    assert_eq!(w.cargo_hubs.stats.forwarded_today, 3);
    let s = w.buildings[buyer].business.as_ref().unwrap().stock_of(Resource::Produce);
    assert!((s - stock0 - 30.0).abs() < 0.01, "all delivered: {s}");
    let st = &w.cargo_hubs.hubs[&h];
    assert!(st.waiting.is_empty() && st.trunk.is_empty() && st.ready.is_empty() && st.trunk_car.is_none());
    assert!(w.buildings[h].business.as_ref().unwrap().money > 0.0, "handling fees");
}

#[test]
fn a_yard_beside_the_hub_unloads_into_it_without_a_lorry() {
    let (mut w, buyer, site) = town();
    w.economy.treasury = 100_000.0;
    let h = w.build_cargo_hub(site).unwrap();
    // Rail freight for the buyer, addressed to the hub.
    let load = hub::HubLoad { buyer, resource: Resource::Parts, amount: 8.0, price: 50.0, since: 0, via: hub::Via::Rail };
    w.cargo_hubs.hubs.get_mut(&h).unwrap().inbound.push(load);
    let trucks0 = w.traffic.cars.len();
    // Unloaded at a building next to the hub (standing in for the yard).
    w.unload_freight(h, Cargo { from: h, to: h, resource: Resource::Parts, amount: 8.0, price: 50.0 });
    assert_eq!(w.traffic.cars.len(), trucks0, "no lorry from the siding");
    assert_eq!(w.cargo_hubs.hubs[&h].ready.len(), 1);
    assert!(w.cargo_hubs.hubs[&h].inbound.is_empty());
    run(&mut w, 5);
    assert_eq!(w.cargo_hubs.stats.forwarded_today, 1, "sent on to the buyer");
}

#[test]
fn a_closed_hub_sends_on_everything_it_held() {
    let (mut w, buyer, site) = town();
    w.economy.treasury = 100_000.0;
    let h = w.build_cargo_hub(site).unwrap();
    for _ in 0..2 {
        w.import(buyer, Resource::Produce, 10.0, 20.0);
    }
    w.buildings[h].business.as_mut().unwrap().closed = true;
    let inbound0 = w.buildings[buyer].business.as_ref().unwrap().inbound_of(Resource::Produce);
    run(&mut w, 2);
    assert!(!w.cargo_hubs.hubs.contains_key(&h));
    // The two loads are on lorries to the buyer (or delivered).
    let on_road = w.traffic.cars.values().filter(|c| c.cargo.is_some_and(|x| x.to == buyer)).count();
    let inbound1 = w.buildings[buyer].business.as_ref().unwrap().inbound_of(Resource::Produce);
    assert!(on_road == 2 || inbound1 < inbound0, "{on_road} lorries, inbound {inbound0} -> {inbound1}");
}

#[test]
fn hub_placement_is_refused_on_parks_and_needs_money() {
    let (mut w, _, site) = town();
    w.economy.treasury = 10.0;
    assert!(w.build_cargo_hub(site).is_err());
    // [free-placement] Homes are no bar any more (the ground leaves the zone); parks are.
    assert!(crate::services::Placeable::CargoHub.allowed_on(LandUse::Residential));
    assert!(!crate::services::Placeable::CargoHub.allowed_on(LandUse::Park));
    assert_eq!(crate::services::Placeable::parse("cargo_hub", None), Some(crate::services::Placeable::CargoHub));
}
