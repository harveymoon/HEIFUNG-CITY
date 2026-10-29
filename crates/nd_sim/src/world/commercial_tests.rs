//! [r4-leisure] Commercial sub-zones: old entertainment saves, the leisure
//! sub-zone and its alias, the player's sub-zones and the city's pick.

use super::*;
use crate::commercial::CommercialSub;
use crate::mind::IdleMind;

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

fn finish_all(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

fn lot_near(w: &World, p: Vec2, lu: LandUse) -> LotId {
    w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == lu).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|(id, _)| id).expect("a vacant lot")
}

fn lots_in(w: &World, poly: &[Vec2]) -> Vec<LotId> {
    w.lots.iter().filter(|(_, l)| l.usable() && point_in_polygon(l.center, poly)).map(|(id, _)| id).collect()
}

#[test]
fn an_old_entertainment_save_loads_as_commercial_leisure_and_its_club_keeps_working() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    // Zoned as the old Entertainment zone was.
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Entertainment);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    let venue_lot = lot_near(&w, Vec2::new(-100.0, 20.0), LandUse::Entertainment);
    let home_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
    w.build_on(venue_lot);
    w.build_on(home_lot);
    let venue = w.lots[venue_lot].building.unwrap();
    finish_all(&mut w);
    // The save as an older build wrote it: no commercial sub-zones at all.
    let text = w.to_ron().unwrap();
    let old: String = text.lines().filter(|l| !l.trim_start().starts_with("commercial_bias:")).collect::<Vec<_>>().join("\n");
    assert!(old.len() < text.len(), "the field is gone from the old save");
    let mut back = World::from_ron(&old).expect("an old save loads");
    assert!(back.commercial_bias.is_empty());
    // Its entertainment lots are commercial land of the leisure sub-zone.
    let ent: Vec<LotId> = back.lots.iter().filter(|(_, l)| l.land_use == LandUse::Entertainment).map(|(id, _)| id).collect();
    assert!(!ent.is_empty());
    for l in &ent {
        assert_eq!(back.lots[*l].land_use.zoned_as(), LandUse::Commercial);
        assert!(back.lots[*l].land_use.is_commercial());
        assert_eq!(back.commercial_sub_set(*l), Some(CommercialSub::Leisure));
    }
    assert_eq!(back.lots[venue_lot].land_use.label(), "Leisure");
    assert_eq!(back.commercial_sub_of_building(venue), Some(CommercialSub::Leisure));
    // The demand for it is commercial demand's leisure part.
    assert_eq!(back.commercial_demand()[CommercialSub::Leisure.index()], back.economy.demand_for(LandUse::Entertainment));
    assert!(back.commercial_demand_overall() >= back.economy.demand_for(LandUse::Entertainment));
    // The club is still a club: open, and someone out for the night is let in.
    assert_eq!(back.buildings[venue].business.as_ref().map(|z| z.kind), Some(BusinessKind::Venue));
    let mut minds = IdleMind;
    for _ in 0..600 {
        if !back.citizens.is_empty() {
            break;
        }
        back.tick(&mut minds);
    }
    let c = back.citizens.keys().next().expect("someone moved in");
    back.citizens[c].savings = 100.0;
    let entrance = back.building_entrance(venue).unwrap();
    back.citizens[c].activity = Activity::Traveling { to: Some(venue), purpose: Purpose::Visit(LandUse::Entertainment), path: vec![entrance], idx: 1 };
    back.arrive(c);
    assert_eq!(back.citizens[c].inside, Some(venue), "the club serves its guests");
}

#[test]
fn painting_commercial_leisure_zones_leisure_land_and_entertainment_is_its_alias() {
    // The old zone's names parse as leisure.
    for s in ["entertainment", "Entertainment", "nightlife", "leisure", "Leisure"] {
        assert_eq!(LandUse::parse(s), Some(LandUse::Entertainment), "{s}");
    }
    assert_eq!(CommercialSub::from_key("entertainment"), Some(CommercialSub::Leisure));
    assert!(!LandUse::ZONED.contains(&LandUse::Entertainment), "no Entertainment zone to pick");
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    let west = square(-200.0, 10.0, 190.0);
    let east = square(10.0, 10.0, 190.0);
    // Commercial, leisure sub-zone: leisure land, as the old zone was.
    let ids = w.parcels.in_polygon(&west);
    let (_, n) = w.paint_parcels_with(&ids, Some(LandUse::Commercial), Some("leisure"), None);
    assert!(n > 0);
    // And the old name straight from the API: the same.
    let ids_e = w.parcels.in_polygon(&east);
    w.paint_parcels_with(&ids_e, LandUse::parse("entertainment"), None, None);
    w.implement();
    for l in lots_in(&w, &west).into_iter().chain(lots_in(&w, &east)) {
        assert_eq!(w.lots[l].land_use, LandUse::Entertainment);
        assert_eq!(w.commercial_sub_set(l), Some(CommercialSub::Leisure));
    }
    // Leisure land painted with the offices sub-zone: commercial offices.
    let (_, n) = w.paint_parcels_with(&ids_e, Some(LandUse::Entertainment), Some("offices"), None);
    assert!(n > 0);
    w.implement();
    let east_lots = lots_in(&w, &east);
    for l in &east_lots {
        assert_eq!(w.lots[*l].land_use, LandUse::Commercial);
        assert_eq!(w.commercial_sub_set(*l), Some(CommercialSub::Offices));
    }
    // Offices land grows offices.
    w.build_on(east_lots[0]);
    let b = w.lots[east_lots[0]].building.unwrap();
    assert_eq!(w.buildings[b].business.as_ref().map(|z| z.kind), Some(BusinessKind::Office));
    assert_eq!(w.commercial_sub_of_building(b), Some(CommercialSub::Offices));
    // Auto hands the ground back to the city.
    w.paint_parcels_with(&ids_e, Some(LandUse::Commercial), Some("auto"), None);
    w.implement();
    assert!(lots_in(&w, &east).iter().all(|l| w.commercial_sub_set(*l).is_none()));
    // The sub-zone is saved with the city.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.commercial_bias, w.commercial_bias);
}

#[test]
fn a_commercial_lot_on_auto_picks_its_sub_zone_from_the_story_and_the_street() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    let lot = lot_near(&w, Vec2::new(-100.0, 20.0), LandUse::Commercial);
    assert_eq!(w.commercial_sub_set(lot), None, "auto by default");
    // A quiet street with no homes and no story, a town with nobody in it
    // yet: a shop or an office, never a club.
    let pick = w.commercial_auto_pick(lot, None).expect("the market wants something");
    assert_ne!(pick, CommercialSub::Leisure);
    // The same lot in a nightlife district (late licence): leisure, so a
    // building there goes up as a club on leisure land.
    let d = w.districts.create(Some("Afterglow".into()), 1, 0).unwrap();
    let at = w.lots[lot].center;
    w.districts.paint_circle(at, 60.0, d);
    w.districts.get_mut(d).unwrap().policies.late_hours = true;
    assert_eq!(w.commercial_auto_pick(lot, None), Some(CommercialSub::Leisure));
    assert_eq!(w.commercial_sub_for_new_building(lot, None), Some(CommercialSub::Leisure));
    w.build_on(lot);
    let b = w.lots[lot].building.unwrap();
    assert_eq!(w.buildings[b].land_use, LandUse::Entertainment);
    assert_eq!(w.buildings[b].business.as_ref().map(|z| z.kind), Some(BusinessKind::Venue));
    assert_eq!(w.lots[lot].land_use, LandUse::Commercial, "the zoning stays commercial on auto");
    assert_eq!(w.commercial_sub_of_building(b), Some(CommercialSub::Leisure));
    // A sub-zone the player set wins over the story.
    let other = lot_near(&w, w.lots[lot].center, LandUse::Commercial);
    w.set_commercial_bias(w.lots[other].polygon.clone(), Some(CommercialSub::Offices)).unwrap();
    assert_eq!(w.commercial_sub_for_new_building(other, None), Some(CommercialSub::Offices));
    // The same world picks the same way every time.
    assert_eq!(w.commercial_auto_pick(lot, None), w.commercial_auto_pick(lot, None));
}
