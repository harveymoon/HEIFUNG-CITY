//! [under-construction] A building that is still a site (or coming down)
//! does nothing as its finished self: no visitors, customers or staff
//! beyond the crew, no walkways, no service cover, no power, no pull on
//! land value. Once it opens, all of it works.

use super::*;
use crate::buildings::Construction;
use crate::mind::IdleMind;
use crate::power::PowerPlantKind;
use crate::services::ServiceKind;

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn run(w: &mut World, n: usize) {
    let mut minds = IdleMind;
    for _ in 0..n {
        w.tick(&mut minds);
    }
}

fn finish_all(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

fn lots_of(w: &World, lu: LandUse) -> Vec<LotId> {
    w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == lu).map(|(id, _)| id).collect()
}

/// A street with homes on both sides, a park, a strip of shops and civic
/// and industrial land; the homes built and lived in.
fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 1e7;
    w.add_road(&[Vec2::new(-500.0, 0.0), Vec2::new(500.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 70.0, 70.0), LandUse::Park);
    w.add_zone(square(100.0, 10.0, 120.0, 60.0), LandUse::Commercial);
    w.add_zone(square(240.0, 10.0, 250.0, 200.0), LandUse::Industrial);
    w.add_zone(square(-490.0, 10.0, 480.0, 60.0), LandUse::Residential);
    w.add_zone(square(-490.0, -70.0, 980.0, 60.0), LandUse::Civic);
    for l in lots_of(&w, LandUse::Residential).into_iter().take(10) {
        w.build_on(l);
    }
    finish_all(&mut w);
    for _ in 0..80 {
        if w.citizens.len() >= 30 {
            break;
        }
        w.immigrate();
    }
    assert!(w.citizens.len() >= 20, "people moved in: {}", w.citizens.len());
    w
}

/// Hold a site where it is: no deliveries come and the crew go home, so a
/// test can watch it for days.
fn freeze(w: &mut World, b: BuildingId) {
    if let Some(c) = w.buildings[b].construction.as_mut() {
        c.progress = 0.5;
        c.deliveries = 0;
        c.ordered = true;
    }
}

#[test]
fn a_park_site_gets_no_visitors_and_no_walkways_until_it_opens() {
    let mut w = town();
    let pl = lots_of(&w, LandUse::Park).into_iter().max_by(|a, b| w.lots[*a].area().total_cmp(&w.lots[*b].area())).expect("a park lot");
    w.build_on(pl);
    let park = w.lots[pl].building.expect("a park site");
    assert!(w.buildings[park].construction.is_some() && !w.buildings[park].operating(), "a park is laid out before it opens");
    freeze(&mut w, park);
    w.refresh_closed_grounds();
    // Its benches are on the walk graph, but shut.
    let bench = w.walk.benches_in(pl).next().map(|b| b.0).expect("the park has benches");
    let out = w.walk.nearest_matching(w.lots[pl].entrance(), 40.0, |n| w.walk.nodes[n as usize].lot != Some(pl)).expect("pavement by the park");
    assert_eq!(w.walk.route_nodes(out, bench), None, "no walk onto the park's paths while it is a site");
    // Nobody picks it, nobody sets off for it.
    let people: Vec<CitizenId> = w.citizens.keys().collect();
    for &id in &people {
        assert_ne!(w.choose_place(id, LandUse::Park, true), Some(park), "a site is nobody's outing");
    }
    for &id in people.iter().take(6) {
        w.citizens[id].activity = Activity::Idle;
        w.citizens[id].inside = None;
        w.travel_to(id, Some(park), Purpose::Visit(LandUse::Park));
        assert!(!w.citizens[id].activity.is_traveling(), "nobody walks to a park site");
    }
    // Two days of town life: nobody sits there, nobody walks its paths.
    for _ in 0..(2 * 1440 / 10) {
        run(&mut w, 10);
        freeze(&mut w, park);
        for c in w.citizens.values() {
            assert!(!matches!(c.activity, Activity::Sitting { park: p, .. } if p == park), "{} sits in a park site", c.name);
            if let Activity::Traveling { to, purpose, path, .. } = &c.activity {
                assert!(!(*to == Some(park) && matches!(purpose, Purpose::Visit(_) | Purpose::Sit)), "{} is off to a park site", c.name);
                let lot = &w.lots[pl].polygon;
                let benches: Vec<Vec2> = w.walk.benches_in(pl).map(|b| w.walk.nodes[b.0 as usize].pos).collect();
                assert!(!path.iter().any(|p| benches.iter().any(|b| b.distance(*p) < 0.5) && crate::geom::point_in_polygon(*p, lot)), "{} walks to a bench in a park site", c.name);
            }
        }
    }
    assert_eq!(w.buildings[park].visitors, 0);
    // Open: on the walk graph, picked, visited, sat in.
    w.finish_construction(park);
    assert!(w.buildings[park].operating());
    assert!(w.walk.route_nodes(out, bench).is_some(), "the paths open with the park");
    let id = people[0];
    w.citizens[id].activity = Activity::Idle;
    w.citizens[id].inside = None;
    w.citizens[id].next_decision_tick = u64::MAX;
    w.travel_to(id, Some(park), Purpose::Visit(LandUse::Park));
    assert!(w.citizens[id].activity.is_traveling(), "off to the park now it is open");
    for _ in 0..300 {
        run(&mut w, 1);
        if matches!(w.citizens[id].activity, Activity::Sitting { .. }) {
            break;
        }
    }
    assert!(matches!(w.citizens[id].activity, Activity::Sitting { park: p, .. } if p == park), "sitting on a bench in the open park: {:?}", w.citizens[id].activity);
}

#[test]
fn a_park_site_lifts_no_land_value_until_it_opens() {
    let mut w = town();
    let pl = lots_of(&w, LandUse::Park).into_iter().max_by(|a, b| w.lots[*a].area().total_cmp(&w.lots[*b].area())).expect("a park lot");
    let at = w.lots[pl].center;
    w.build_on(pl);
    let park = w.lots[pl].building.unwrap();
    w.update_land_value();
    let site = w.land_value.at(at);
    w.finish_construction(park);
    w.update_land_value();
    let open = w.land_value.at(at);
    assert!(open > site + 0.05, "the open park lifts the land, the site did not: {site} -> {open}");
}

#[test]
fn a_service_under_construction_covers_nothing_and_runs_no_units() {
    let mut w = town();
    let lot = lots_of(&w, LandUse::Civic).into_iter().filter(|l| w.lots[*l].car_access() && w.lots[*l].area() >= 420.0).min_by(|a, b| w.lots[*a].center.x.abs().total_cmp(&w.lots[*b].center.x.abs())).expect("a civic lot");
    let station = w.build_service(lot, ServiceKind::Fire, Tier::Mid).expect("a fire station");
    let home = w.buildings.values().find(|b| b.homes > 0).map(|b| b.lot).unwrap();
    w.refresh_coverage();
    assert!(w.services.coverage_of(home, ServiceKind::Fire) > 0.0, "an open station covers the street");
    let b = &mut w.buildings[station];
    b.construction = Some(Construction::new(2.0));
    b.recompute_capacity();
    w.refresh_coverage();
    assert_eq!(w.services.coverage_of(home, ServiceKind::Fire), 0.0, "a station still being built covers nobody");
    assert_eq!(w.crewed_units(station), 0, "and puts no engine on the road");
    // The signature sees it open, so the cover comes back on its own.
    w.finish_construction(station);
    run(&mut w, 20);
    assert!(w.services.coverage_of(home, ServiceKind::Fire) > 0.0, "covered again once it opens");
}

#[test]
fn a_plant_under_construction_produces_nothing() {
    let mut w = town();
    let lot = lots_of(&w, LandUse::Industrial).into_iter().max_by(|a, b| w.lots[*a].area().total_cmp(&w.lots[*b].area())).expect("an industrial lot");
    let plant = w.build_power(lot, PowerPlantKind::Coal).expect("a coal plant");
    assert!(w.plant_output_mw(&w.buildings[plant]) > 0.0, "an open plant feeds the grid");
    let b = &mut w.buildings[plant];
    b.construction = Some(Construction::new(2.0));
    b.recompute_capacity();
    assert_eq!(w.plant_output_mw(&w.buildings[plant]), 0.0, "a plant site feeds nothing");
    w.allocate_power();
    assert_eq!(w.economy.power.supply_by_kind[PowerPlantKind::Coal.index()], 0.0);
    w.finish_construction(plant);
    assert!(w.plant_output_mw(&w.buildings[plant]) > 0.0);
    assert!(w.economy.power.supply_by_kind[PowerPlantKind::Coal.index()] > 0.0, "the grid takes it in as it opens");
}

#[test]
fn a_shop_under_construction_hires_only_its_crew_and_serves_nobody() {
    let mut w = town();
    let lot = lots_of(&w, LandUse::Commercial)[0];
    let shop = w.build_on_as(lot, Some(BusinessKind::Bakery)).expect("a bakery site");
    freeze(&mut w, shop);
    let b = &w.buildings[shop];
    assert!(b.business.is_some() && !b.operating());
    assert_eq!(b.jobs, Construction::crew(b.floors), "a site's only jobs are the crew's");
    assert!(!b.takes_customers() && !(0..24).any(|h| b.serves_at(h)), "a site serves nobody");
    for _ in 0..40 {
        if let Some(j) = w.pick_job() {
            assert!(w.buildings[j].operating() || w.buildings[j].construction.is_some());
            if j == shop {
                assert_eq!(w.buildings[j].wage(), Building::SITE_WAGE, "hired onto the crew");
            }
        }
    }
    let id = w.citizens.keys().next().unwrap();
    w.citizens[id].activity = Activity::Idle;
    w.citizens[id].inside = None;
    w.travel_to(id, Some(shop), Purpose::Shop(crate::market::Resource::BakedGoods));
    assert!(!w.citizens[id].activity.is_traveling(), "nobody goes shopping at a site");
    assert_ne!(w.choose_shop(id, crate::market::Resource::BakedGoods, w.citizens[id].pos), Some(shop));
    w.finish_construction(shop);
    let b = &w.buildings[shop];
    assert!(b.operating() && b.jobs > 0 && b.wage() != Building::SITE_WAGE);
    assert!((0..24).any(|h| b.serves_at(h)), "open, it serves in its hours");
}

#[test]
fn a_service_on_a_lot_still_being_cleared_waits_for_the_clearing() {
    let mut w = town();
    // A shop stands where the city puts a clinic: the clinic works once
    // the shop is down.
    let lot = lots_of(&w, LandUse::Civic).into_iter().filter(|l| w.lots[*l].car_access() && w.lots[*l].area() >= 420.0).next().unwrap();
    w.build_on(lot);
    finish_all(&mut w);
    let old = w.lots[lot].building.unwrap();
    // Far enough up that it comes down as a demolition site.
    w.buildings[old].floors = w.buildings[old].floors.max(3);
    let clinic = w.build_service(lot, ServiceKind::Health, Tier::Mid).expect("a clinic");
    if w.buildings[clinic].demolition.is_none() {
        // Cleared at once on this ground: nothing to wait for.
        return;
    }
    assert!(!w.buildings[clinic].operating(), "the lot is still being cleared");
    let home = w.buildings.values().find(|b| b.homes > 0).map(|b| b.lot).unwrap();
    w.refresh_coverage();
    assert_eq!(w.services.coverage_of(home, ServiceKind::Health), 0.0);
    assert_ne!(w.pick_job(), Some(clinic), "it hires once it opens");
    w.finish_demolition(clinic);
    assert!(w.buildings[clinic].operating());
    w.refresh_coverage();
    assert!(w.services.coverage_of(home, ServiceKind::Health) > 0.0);
}
