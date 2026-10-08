//! [districts] [tourism] District policies, the facelift, the Historic
//! freeze, and visitors from arrival to the tourist tax.

use super::districts::PolicyChange;
use super::*;
use crate::districts::{CommercialSpec, Look, ResidentialSpec};
use crate::industry::IndustryKind;
use crate::mind::IdleMind;
use crate::services::ServiceKind;
use crate::tourism::{Arrival, AttractionKind};

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), dead_ends_lead_elsewhere: true, ..WorldConfig::default() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w
}

fn finish_all(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

fn vacant(w: &World, lu: LandUse) -> Vec<LotId> {
    let mut v: Vec<(LotId, f32)> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == lu).map(|(id, l)| (id, l.center.x)).collect();
    v.sort_by(|a, b| a.1.total_cmp(&b.1));
    v.into_iter().map(|(id, _)| id).collect()
}

/// A district over the whole test street.
fn district_over_street(w: &mut World) -> u16 {
    let id = w.create_district(Some("Lantern Row".into())).unwrap();
    w.paint_district_polygon(&square(-400.0, -220.0, 800.0), id).unwrap();
    assert!(w.districts.at(Vec2::new(50.0, 30.0)).is_some());
    id
}

#[test]
fn a_district_policy_shapes_new_buildings() {
    let mut w = town();
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.add_zone(square(10.0, -200.0, 190.0), LandUse::Commercial);
    w.land_value.fill(0.9);
    let id = district_over_street(&mut w);
    w.set_district_policy(id, PolicyChange { residential: Some(Some(ResidentialSpec::Kowloon)), commercial: Some(Some(CommercialSpec::TechCampus)), ..Default::default() }).unwrap();
    for lot in vacant(&w, LandUse::Residential).into_iter().take(3) {
        w.build_on(lot);
        let b = &w.buildings[w.lots[lot].building.unwrap()];
        assert_eq!(b.look, Some(Look::Kowloon));
        assert!(b.tier <= Tier::Mid, "the stacks are cheap even on dear land");
    }
    w.land_value.fill(0.5);
    let lot = vacant(&w, LandUse::Commercial)[0];
    w.build_on(lot);
    let b = &w.buildings[w.lots[lot].building.unwrap()];
    assert_eq!(b.look, Some(Look::TechCampus));
    assert_eq!(b.tier, Tier::High, "a campus builds a step up");
    // Outside any district nothing changes.
    w.remove_district(id);
    let lot = vacant(&w, LandUse::Residential)[0];
    w.build_on(lot);
    assert_eq!(w.buildings[w.lots[lot].building.unwrap()].look, None);
}

#[test]
fn standing_buildings_take_the_look_over_days_and_historic_keeps_them() {
    let mut w = town();
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    w.land_value.fill(0.5);
    let lots = vacant(&w, LandUse::Residential);
    for lot in &lots {
        w.build_on(*lot);
    }
    finish_all(&mut w);
    let n = w.buildings.len();
    assert!(n >= 4);
    let id = district_over_street(&mut w);
    w.set_district_policy(id, PolicyChange { residential: Some(Some(ResidentialSpec::Eco)), ..Default::default() }).unwrap();
    for _ in 0..12 {
        w.clock.tick += 1440;
        w.daily_districts();
    }
    let eco = w.buildings.values().filter(|b| b.look == Some(Look::Eco)).count();
    assert_eq!(eco, n, "every home refitted to eco within twelve days");
    // Historic: tiers freeze even on land that would redevelop them, and
    // nothing is refitted.
    w.set_district_policy(id, PolicyChange { residential: Some(Some(ResidentialSpec::Kowloon)), historic: Some(true), ..Default::default() }).unwrap();
    let before: Vec<(BuildingId, Tier, u64)> = w.buildings.iter().map(|(id, b)| (id, b.tier, b.seed)).collect();
    w.land_value.fill(0.97);
    for b in w.buildings.values_mut() {
        b.high_land_days = REDEVELOP_DAYS;
    }
    for _ in 0..5 {
        w.clock.tick += 1440;
        w.daily_update();
        w.daily_districts();
    }
    for (id, tier, seed) in before {
        let b = &w.buildings[id];
        assert_eq!((b.tier, b.seed), (tier, seed), "historic buildings stay as they stand");
        assert!(b.demolition.is_none() && b.look == Some(Look::Eco));
    }
}

#[test]
fn a_district_industry_policy_is_a_bias_that_follows_its_cells() {
    let mut w = town();
    let id = district_over_street(&mut w);
    assert_eq!(w.industry_bias_at(Vec2::new(50.0, 30.0)), None);
    w.set_district_policy(id, PolicyChange { industry: Some(Some(IndustryKind::Farming)), ..Default::default() }).unwrap();
    assert_eq!(w.industry_bias_at(Vec2::new(50.0, 30.0)), Some(Some(IndustryKind::Farming)));
    assert_eq!(w.industry_bias_at(Vec2::new(900.0, 900.0)), None);
    // A drawn bias still wins over the district's.
    w.set_industry_bias(IndustryArea::Polygon(square(0.0, 0.0, 100.0)), Some(IndustryKind::Tech)).unwrap();
    assert_eq!(w.industry_bias_at(Vec2::new(50.0, 30.0)), Some(Some(IndustryKind::Tech)));
}

#[test]
fn district_stats_count_what_is_inside() {
    let mut w = town();
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    for lot in vacant(&w, LandUse::Residential).into_iter().take(3) {
        w.build_on(lot);
    }
    finish_all(&mut w);
    let id = district_over_street(&mut w);
    let stats = w.district_stats();
    let s = stats.iter().find(|s| s.id == id).unwrap();
    assert_eq!(s.buildings, 3);
    assert!(s.homes > 0 && s.lots >= 3 && s.area_m2 > 100_000.0);
    assert_eq!(s.by_land_use[LandUse::Residential.index()], 3);
}

/// A town worth visiting: a monument and shops along the street, a way in
/// at the dead ends, and commercial land for hotels.
fn tourist_town() -> World {
    let mut w = town();
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Commercial);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Civic);
    w.add_zone(square(10.0, -200.0, 190.0), LandUse::Commercial);
    w.land_value.fill(0.55);
    let civic = vacant(&w, LandUse::Civic)[0];
    w.economy.treasury = 1e7;
    w.build_service(civic, ServiceKind::Monument, Tier::High).expect("a monument");
    let shop = vacant(&w, LandUse::Commercial)[0];
    w.build_on(shop);
    finish_all(&mut w);
    w
}

#[test]
fn attractions_draw_visitors_who_need_beds_and_hotels_open_for_them() {
    let mut w = tourist_town();
    w.refresh_attractions();
    let t = &w.tourism;
    assert!(t.attractions.iter().any(|a| a.kind == AttractionKind::Monument));
    assert!(t.attractiveness > 20.0 && t.ways_in[Arrival::Road.index()]);
    assert!(t.demand_per_day > 5.0, "demand {}", t.demand_per_day);
    // No hotel: everyone who came is turned away, which is the demand that
    // opens one.
    let mut minds = IdleMind;
    while w.clock.hour() != 23 {
        w.tick(&mut minds);
    }
    assert!(w.tourism.stats.turned_away_today > 0 && w.tourism.tourists.is_empty());
    for _ in 0..(3 * 1440) {
        w.tick(&mut minds);
        if w.buildings.values().any(|b| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Hotel)) {
            break;
        }
    }
    let hotel = w.buildings.iter().find(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Hotel)).map(|(id, _)| id).expect("a hotel opened for the visitors");
    assert_eq!(w.buildings[hotel].land_use, LandUse::Commercial);
    w.finish_construction(hotel);
    assert!(w.hotel_beds(hotel) > 0);
}

#[test]
fn visitors_stay_spend_pay_the_tourist_tax_and_leave() {
    let mut w = tourist_town();
    let lot = vacant(&w, LandUse::Commercial)[0];
    w.build_on(lot);
    let hotel = w.lots[lot].building.unwrap();
    w.buildings[hotel].business = Some(Business::new(BusinessKind::Hotel, w.buildings[hotel].jobs));
    w.finish_construction(hotel);
    let beds = w.hotel_beds(hotel);
    assert!(beds >= 8);
    let mut minds = IdleMind;
    let mut seen_outside = false;
    let mut most = 0;
    let mut tax = 0.0;
    let mut nights = 0;
    for _ in 0..(5 * 1440) {
        w.tick(&mut minds);
        most = most.max(w.tourism.count());
        seen_outside |= w.tourism.tourists.values().any(|t| t.is_visible());
        if w.clock.minute_of_day() == 1 {
            tax += w.tourism.stats.tax_today;
            nights += w.tourism.stats.nights_today;
        }
        assert!(w.hotel_guests().get(&hotel).copied().unwrap_or(0) <= beds, "never more guests than beds");
    }
    assert!(most > 0, "visitors came");
    assert!(seen_outside, "visitors were out on the street");
    assert!(nights > 0 && tax > 0.0, "nights {nights}, tax {tax}");
    assert!(w.economy.ledger.yesterday_of(Flow::TouristTax) > 0.0 || w.economy.ledger.today_of(Flow::TouristTax) > 0.0);
    assert!(w.tourism.stats.arrivals_total as usize >= most);
    assert!(w.tourism.stats.spend_total > 0.0);
    // Stays are short: after five days some have gone home.
    let departed = w.tourism.stats.arrivals_total as usize - w.tourism.count();
    assert!(departed > 0, "some left");
    // Saves keep them.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.tourism.count(), w.tourism.count());
    assert_eq!(back.districts.list.len(), w.districts.list.len());
}

#[test]
fn tourism_is_deterministic() {
    let run = || {
        let mut w = tourist_town();
        let lot = vacant(&w, LandUse::Commercial)[0];
        w.build_on(lot);
        let hotel = w.lots[lot].building.unwrap();
        w.buildings[hotel].business = Some(Business::new(BusinessKind::Hotel, w.buildings[hotel].jobs));
        w.finish_construction(hotel);
        let mut minds = IdleMind;
        for _ in 0..(2 * 1440) {
            w.tick(&mut minds);
        }
        (w.tourism.stats.arrivals_total, w.tourism.count(), w.tourism.tourists.values().map(|t| (t.pos.x * 10.0) as i64).sum::<i64>())
    };
    assert_eq!(run(), run());
}

/// [review] A merged building spans several lots and was counted once per
/// lot in the district figures.
#[test]
fn a_merged_building_counts_once_in_its_district() {
    let mut w = town();
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    let lots = vacant(&w, LandUse::Residential);
    w.build_on(lots[0]);
    finish_all(&mut w);
    let b = w.lots[lots[0]].building.unwrap();
    // Spread over two more plots.
    for &l in &lots[1..3] {
        w.buildings[b].plot.extra_lots.push(l);
        w.lots[l].building = Some(b);
    }
    let id = district_over_street(&mut w);
    let stats = w.district_stats();
    let s = stats.iter().find(|s| s.id == id).unwrap();
    assert_eq!(s.buildings, 1);
    assert_eq!(s.homes, w.buildings[b].homes as u32);
    assert_eq!(s.residents, w.buildings[b].residents.len() as u32);
}
