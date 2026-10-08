//! [economy] Waterfront tests: marinas and fishing ports go on the shore
//! only, are run by an operator, lift the land, send boats out on the
//! water and come back.

use super::*;
use crate::landscape::{LandscapeParams, LandscapePreset};
use crate::mind::IdleMind;
use crate::ports::{PortKind, ShipKind};

fn valley(seed: u64) -> World {
    World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::RiverValley, seed), ..WorldConfig::default() })
}

/// A point on the main river inside the basin, its flow, the unit vector
/// towards the middle of the map, and its width.
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

/// A riverside street with homes behind it, and the click for a dock.
fn riverside() -> (World, Vec2, Vec2, Vec2) {
    let mut w = valley(8);
    w.economy.treasury = 500_000.0;
    let (p, flow, n, width) = river_spot(&w);
    let road_at = p + n * (width * 0.5 + 110.0);
    w.add_road(&[road_at - flow * 400.0, road_at + flow * 400.0], 1, 1);
    w.add_zone(vec![road_at + n * 10.0 - flow * 150.0, road_at + n * 10.0 + flow * 150.0, road_at + n * 90.0 + flow * 150.0, road_at + n * 90.0 - flow * 150.0], LandUse::Residential);
    (w, p + n * (width * 0.5 + 20.0), flow, road_at)
}

fn join_stub(w: &mut World, id: BuildingId, road_at: Vec2, flow: Vec2) {
    let dock = w.ports.port_of(id).unwrap().clone();
    let [s0, _] = dock.road_line();
    w.add_road(&[s0, road_at - flow * 400.0], 1, 1);
}

#[test]
fn marinas_and_fishing_ports_only_on_the_shore() {
    let (mut w, click, flow, road_at) = riverside();
    for k in [PortKind::MarinaSmall, PortKind::MarinaLarge, PortKind::FishingPortSmall, PortKind::FishingPortLarge] {
        let e = w.place_port(k, Vec2::new(0.0, 0.0)).unwrap_err();
        assert!(e.contains("water's edge"), "{k:?} inland: {e}");
    }
    let marina = w.place_port(PortKind::MarinaLarge, click).expect("a marina on the river bank");
    let dock = w.ports.port_of(marina).unwrap().clone();
    assert!(w.landscape.is_water(dock.pier[1]), "its pontoon reaches the water");
    assert!(!w.landscape.is_water(dock.center), "its yard is on land");
    assert!(dock.corners().iter().all(|c| !w.landscape.is_water(*c)));
    join_stub(&mut w, marina, road_at, flow);
    let b = &w.buildings[marina];
    assert_eq!(b.port, Some(PortKind::MarinaLarge));
    assert_eq!(b.land_use, LandUse::Commercial);
    let z = b.business.as_ref().expect("an operator runs it");
    assert_eq!(z.kind, BusinessKind::Marina);
    assert!(z.origin.is_some());
    let fishing = w.place_port(PortKind::FishingPortSmall, click + flow * 220.0).or_else(|_| w.place_port(PortKind::FishingPortSmall, click - flow * 220.0)).expect("a fishing port along the bank");
    let fp = w.ports.port_of(fishing).unwrap().footprint();
    assert!(!crate::zoning::polygons_overlap(&fp, &dock.footprint()));
    assert_eq!(w.buildings[fishing].business.as_ref().unwrap().kind, BusinessKind::FishingPort);
    assert_eq!(w.buildings[fishing].land_use, LandUse::Industrial);
}

#[test]
fn a_marina_lifts_the_land_and_its_boats_sail_on_the_water() {
    let (mut w, click, flow, road_at) = riverside();
    w.update_land_value();
    let probe = click + (road_at - click) * 0.5;
    let before = w.land_value.at(probe);
    let marina = w.place_port(PortKind::MarinaLarge, click).unwrap();
    join_stub(&mut w, marina, road_at, flow);
    assert!(w.ports.port_of(marina).unwrap().works(), "the marina works");
    w.update_land_value();
    let after = w.land_value.at(probe);
    assert!(after > before + 0.03, "a marina lifts the land around it: {before} -> {after}");
    // Workers for the operator, and a morning on the water.
    let jobs = w.buildings[marina].jobs as usize;
    w.buildings[marina].workers = vec![CitizenId::default(); jobs];
    let mut minds = IdleMind;
    while w.clock.hour() != 9 {
        w.tick(&mut minds);
    }
    let mut seen = false;
    for _ in 0..(5 * 60) {
        w.tick(&mut minds);
        for s in w.ports.ships.values().filter(|s| matches!(s.kind, ShipKind::Yacht | ShipKind::Sailboat)) {
            seen = true;
            assert_eq!(s.port, marina);
            for p in &s.path {
                assert!(w.landscape.is_water(*p), "boats keep to the water: {p:?}");
            }
        }
    }
    assert!(seen, "boats went out from the marina");
}

#[test]
fn a_fishing_port_s_trawlers_go_out_in_the_morning_and_land_the_catch() {
    let (mut w, click, flow, road_at) = riverside();
    let port = w.place_port(PortKind::FishingPortLarge, click).unwrap();
    join_stub(&mut w, port, road_at, flow);
    let jobs = w.buildings[port].jobs as usize;
    w.buildings[port].workers = vec![CitizenId::default(); jobs];
    let mut minds = IdleMind;
    while w.clock.hour() != 2 {
        w.tick(&mut minds);
    }
    let mut out = 0;
    let mut landed = 0.0f32;
    for _ in 0..(18 * 60) {
        w.tick(&mut minds);
        out = out.max(w.fleet(port).map_or(0, |f| f.at_sea()));
        landed = landed.max(w.scene.fish_landed_today);
        w.buildings[port].workers = vec![CitizenId::default(); jobs];
    }
    let f = w.fleet(port).expect("a fleet");
    assert!(!f.grounds.is_empty(), "grounds on the river");
    assert!(out > 0, "trawlers put out");
    assert!(landed > 0.0, "and came back with fish");
    assert!(w.ports.ships.values().all(|s| s.kind != ShipKind::FishingBoat), "trawlers are the fleet's, not port ships");
    let z = w.buildings[port].business.as_ref().unwrap();
    assert!(z.stock_of(Resource::Fish) > 0.0 || z.deals > 0 || z.revenue_today > 0.0, "the quay has fish to sell");
}

#[test]
fn an_operator_that_goes_under_is_replaced_and_the_dock_stays() {
    let (mut w, click, flow, road_at) = riverside();
    let marina = w.place_port(PortKind::MarinaSmall, click).unwrap();
    join_stub(&mut w, marina, road_at, flow);
    let z = w.buildings[marina].business.as_mut().unwrap();
    z.closed = true;
    z.broke_days = 3;
    w.waterfront_daily();
    let z = w.buildings[marina].business.as_ref().unwrap();
    assert!(!z.closed, "a new operator took over");
    assert!(z.origin.as_deref().is_some_and(|o| o.contains("new operator")));
    assert!(w.ports.port_of(marina).is_some());
}
