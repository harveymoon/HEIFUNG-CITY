//! [free-placement] City buildings on unzoned ground, the ground under
//! them out of the zoning, back to the plan when they go.

use super::*;
use crate::power::PowerPlantKind;
use crate::services::{Placeable, ServiceKind};

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

/// An avenue with housing zoned along its south side and nothing zoned to
/// the north, flat ground, plenty of money.
fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 5_000_000.0;
    w.add_road(&[Vec2::new(-600.0, 0.0), Vec2::new(600.0, 0.0)], 1, 1);
    w.add_zone(square(-590.0, -60.0, 1180.0, 52.0), LandUse::Residential);
    w
}

fn lots_of(w: &World, lu: LandUse) -> usize {
    w.lots.values().filter(|l| l.land_use == lu && !w.is_city_ground(l)).count()
}

/// A point on unzoned ground north of the avenue, on a parcel.
fn unzoned_point(w: &World, x: f32) -> Vec2 {
    let p = Vec2::new(x, 14.0);
    let i = w.parcels.at(p).expect("a parcel north of the avenue");
    assert!(w.parcels.parcels[i].lot.is_none() && w.parcels.parcels[i].land_use.is_none(), "unzoned");
    p
}

/// A vacant residential lot by the avenue, nearest `x`.
fn home_lot(w: &World, x: f32) -> LotId {
    w.lots
        .iter()
        .filter(|(_, l)| l.land_use == LandUse::Residential && l.usable() && l.building.is_none() && l.car_access())
        .min_by(|a, b| (a.1.center.x - x).abs().total_cmp(&(b.1.center.x - x).abs()))
        .map(|(id, _)| id)
        .expect("a residential lot")
}

#[test]
fn a_plant_goes_on_unzoned_ground_and_the_ground_becomes_the_citys() {
    let mut w = town();
    let lots0 = w.lots.len();
    let p = unzoned_point(&w, -300.0);
    let site = w.placement_problem_at(Placeable::Plant(PowerPlantKind::Solar), p).expect("unzoned ground takes a solar farm");
    assert!(site.lot.is_none() && !site.parcels.is_empty());
    assert!(site.note().contains("unzoned"), "{}", site.note());
    let id = w.place_building(Placeable::Plant(PowerPlantKind::Solar), p).expect("placed");
    assert_eq!(w.lots.len(), lots0 + 1, "a lot was made for it");
    let b = &w.buildings[id];
    assert_eq!(b.land_use, LandUse::Industrial, "a plant works as industry whatever the ground");
    assert_eq!(w.shown_use(id), LandUse::Civic, "and reads Civic: the ground is the city's");
    let lot = &w.lots[b.lot];
    assert_eq!(lot.land_use, LandUse::Civic);
    assert!(w.is_city_ground(lot));
    assert_eq!(w.zones[lot.zone].polygon, lot.polygon, "a zone of its own");
    for &i in &site.parcels {
        assert_eq!(w.parcels.parcels[i].lot, Some(b.lot));
        assert_eq!(w.parcels.parcels[i].land_use, Some(LandUse::Civic));
    }

    // A coal plant wants more ground than a parcel: unzoned neighbours join.
    let q = unzoned_point(&w, 300.0);
    let site = w.placement_site(Placeable::Plant(PowerPlantKind::Coal), q).unwrap();
    assert!(site.parcels.len() > 1, "parcels joined for a works-sized site");
    let coal = w.place_building(Placeable::Plant(PowerPlantKind::Coal), q).expect("coal on unzoned ground");
    assert!(w.lots[w.buildings[coal].lot].area() >= PowerPlantKind::Coal.min_area());

    // Bulldozed: the lot goes, the parcels are unzoned again.
    let lot_id = w.buildings[id].lot;
    let parcels = w.parcels.in_polygon(&w.lots[lot_id].polygon);
    w.bulldoze_building(id).expect("the city's own");
    assert!(!w.lots.contains_key(lot_id));
    for i in parcels {
        assert!(w.parcels.parcels[i].lot.is_none() && w.parcels.parcels[i].land_use.is_none());
    }
    assert!(w.placement_problem_at(Placeable::Plant(PowerPlantKind::Solar), p).is_ok(), "free again");
}

#[test]
fn a_police_station_on_housing_is_civic_and_goes_back_to_housing_when_bulldozed() {
    let mut w = town();
    let homes0 = lots_of(&w, LandUse::Residential);
    let lot = home_lot(&w, 0.0);
    let centre = w.lots[lot].center;
    let site = w.placement_problem_at(Placeable::Service(ServiceKind::Police, Tier::Low), centre).expect("homes are no bar");
    assert_eq!(site.leaves_zone, Some(LandUse::Residential));
    assert!(site.note().contains("residential"), "{}", site.note());
    let id = w.place_on_lot(Placeable::Service(ServiceKind::Police, Tier::Low), lot).expect("placed");
    assert_eq!(w.buildings[id].lot, lot, "same lot, nothing moved");
    assert_eq!(w.buildings[id].land_use, LandUse::Civic, "a police station, not a home");
    assert_eq!(w.shown_use(id), LandUse::Civic);
    assert_eq!(w.lots[lot].land_use, LandUse::Civic);
    assert_eq!(lots_of(&w, LandUse::Residential), homes0 - 1, "the zone has one lot fewer");
    let parcel = w.parcels.at(centre).unwrap();
    assert!(w.parcel_refusal(parcel, Some(LandUse::Residential)).is_some(), "the zone tools leave it alone");
    let (_, n) = w.paint_parcels(&[parcel], Some(LandUse::Commercial), None);
    assert_eq!(n, 0);

    // A rebuild (a road elsewhere) keeps it the city's.
    w.add_road(&[Vec2::new(-600.0, 200.0), Vec2::new(600.0, 200.0)], 1, 1);
    let lot = w.buildings[id].lot;
    assert_eq!(w.lots[lot].land_use, LandUse::Civic);
    assert_eq!(lots_of(&w, LandUse::Residential), homes0 - 1);

    // Bulldozed: back to housing, the zone whole again.
    w.bulldoze_building(id).expect("bulldozed");
    assert!(!w.buildings.contains_key(id));
    let back = w.lots.iter().find(|(_, l)| l.contains(centre)).map(|(_, l)| l.land_use);
    assert_eq!(back, Some(LandUse::Residential));
    assert_eq!(lots_of(&w, LandUse::Residential), homes0);
    w.add_road(&[Vec2::new(-600.0, -200.0), Vec2::new(600.0, -200.0)], 1, 1);
    assert_eq!(lots_of(&w, LandUse::Residential), homes0, "and stays so through a rebuild");
}

#[test]
fn grown_buildings_are_not_bulldozed_and_parks_still_refuse() {
    let mut w = town();
    let lot = home_lot(&w, 100.0);
    w.build_on(lot);
    let home = w.lots[lot].building.unwrap();
    assert!(w.bulldoze_building(home).is_err());
    w.add_zone(square(-200.0, 8.0, 120.0, 50.0), LandUse::Park);
    let park = w.lots.iter().find(|(_, l)| l.land_use == LandUse::Park && l.usable()).map(|(id, _)| id).unwrap();
    let err = w.place_on_lot(Placeable::Plant(PowerPlantKind::Solar), park).unwrap_err();
    assert!(err.contains("park"), "{err}");
}

#[test]
fn an_older_save_keeps_every_building_and_switches_city_lots_on_load() {
    let mut w = town();
    let lot = home_lot(&w, -100.0);
    let station = w.place_on_lot(Placeable::Service(ServiceKind::Dispatch, Tier::Low), lot).unwrap();
    let plant = w.place_building(Placeable::Plant(PowerPlantKind::Solar), unzoned_point(&w, 250.0)).unwrap();
    let home = home_lot(&w, 200.0);
    w.build_on(home);
    // As an older save had it: the station's lot still the zone's.
    let zone = w.lots[home].zone;
    w.lots[lot].land_use = LandUse::Residential;
    w.lots[lot].zone = zone;
    w.buildings[station].land_use = LandUse::Residential;
    let before: Vec<(BuildingId, Vec2)> = w.buildings.iter().map(|(id, b)| (id, w.lots[b.lot].center)).collect();

    let mut loaded = World::from_ron(&w.to_ron().unwrap()).unwrap();
    for (id, at) in &before {
        let b = loaded.buildings.get(*id).expect("every building is still there");
        assert!(loaded.lots[b.lot].center.distance(*at) < 0.01, "nothing moved");
    }
    let l = &loaded.lots[loaded.buildings[station].lot];
    assert_eq!(l.land_use, LandUse::Civic);
    assert_eq!(loaded.shown_use(station), LandUse::Civic);
    assert_eq!(loaded.buildings[station].land_use, LandUse::Residential, "what it does is not changed by a load");
    assert!(loaded.is_city_ground(&loaded.lots[loaded.buildings[plant].lot]));
    // And through the first rebuild.
    loaded.add_road(&[Vec2::new(-600.0, 200.0), Vec2::new(600.0, 200.0)], 1, 1);
    for (id, _) in &before {
        assert!(loaded.buildings.contains_key(*id), "a rebuild keeps every building");
    }
    assert_eq!(loaded.lots[loaded.buildings[station].lot].land_use, LandUse::Civic);
}

#[test]
fn a_city_building_keeps_its_own_use_whatever_the_ground() {
    let mut w = town();
    // A depot and a stadium on housing, a cargo hub on unzoned ground.
    let lot = home_lot(&w, 200.0);
    let depot = w.place_building(Placeable::BusDepot, unzoned_point(&w, -450.0)).expect("depot on unzoned ground");
    assert_eq!(w.buildings[depot].land_use, LandUse::Industrial, "a depot draws and pays like industry");
    let stadium = w.place_building(Placeable::Service(ServiceKind::Stadium, Tier::Low), unzoned_point(&w, 450.0)).expect("stadium on unzoned ground");
    assert_eq!(w.buildings[stadium].land_use, LandUse::Entertainment, "a stadium is a night out");
    let dispatch = w.place_on_lot(Placeable::Service(ServiceKind::Dispatch, Tier::Low), lot).expect("dispatch on housing");
    assert_eq!(w.buildings[dispatch].land_use, LandUse::Civic);
    for id in [depot, stadium, dispatch] {
        assert_eq!(w.shown_use(id), LandUse::Civic);
        assert_eq!(w.lots[w.buildings[id].lot].land_use, LandUse::Civic);
    }
    let draw = crate::power::demand_mw(&w.buildings[depot]);
    w.add_road(&[Vec2::new(-600.0, 200.0), Vec2::new(600.0, 200.0)], 1, 1);
    assert_eq!(w.buildings[depot].land_use, LandUse::Industrial, "a rebuild keeps it");
    assert_eq!(crate::power::demand_mw(&w.buildings[depot]), draw);
}
