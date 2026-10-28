//! [stories] District stories: old saves' districts take their story,
//! a story sets the building types and its policies, and the policies move
//! real money through the ledger's door and change what the sim does.

use super::districts::PolicyChange;
use super::*;
use crate::districts::{CommercialSpec, District, ResidentialSpec};
use crate::mind::IdleMind;
use crate::stories::{Policy, Story};

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

/// A district over the whole of a loaded city.
fn district_over_all(w: &mut World, name: &str) -> u16 {
    let id = w.create_district(Some(name.into())).unwrap();
    let half = crate::landscape::MAP_EDGE;
    w.paint_district_polygon(&square(-half, -half, 2.0 * half), id).unwrap();
    id
}

/// [stories] A save from before stories: a district whose dials are
/// exactly a story's takes it (keeping its own height limit, getting the
/// story's policies), one whose dials no story has stays custom (with the
/// nearest story suggested), one with no dials has no story; a district
/// as saved before the fields existed still loads.
#[test]
fn old_save_districts_take_the_story_their_dials_are() {
    let old = "(id: 3, name: \"Jade Row\", color: 2, residential: Some(Eco), commercial: Some(TechCampus), industry: None, historic: false, height_limit: Some(90.0))";
    let d: District = ron::from_str(old).expect("an old district loads");
    assert_eq!(d.story, None);
    assert!(d.is_custom() && d.policies.is_none());
    assert_eq!(Story::matching(&d), Some(Story::FoundersCampus));

    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
    let set = |w: &mut World, name: &str, change: PolicyChange| {
        let id = w.create_district(Some(name.into())).unwrap();
        w.set_district_policy(id, change).unwrap();
        id
    };
    let campus = set(&mut w, "Jade Terraces", PolicyChange { residential: Some(Some(ResidentialSpec::Eco)), commercial: Some(Some(CommercialSpec::TechCampus)), height_limit: Some(Some(90.0)), ..Default::default() });
    let strip = set(&mut w, "Iron Flats", PolicyChange { commercial: Some(Some(CommercialSpec::NeonStrip)), ..Default::default() });
    let odd = set(&mut w, "Signal Wharf", PolicyChange { residential: Some(Some(ResidentialSpec::Kowloon)), commercial: Some(Some(CommercialSpec::Tourist)), ..Default::default() });
    let plain = set(&mut w, "Static Gardens", PolicyChange { height_limit: Some(Some(90.0)), ..Default::default() });
    // Dials set by hand leave the districts with no story, as an old save's.
    assert!(w.districts.list.iter().all(|d| d.story.is_none()));
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    let get = |id: u16| back.districts.get(id).unwrap();
    assert_eq!(get(campus).story, Some(Story::FoundersCampus));
    assert_eq!(get(campus).height_limit, Some(90.0), "its own height limit kept");
    assert_eq!(get(campus).policies, Story::FoundersCampus.preset().policies, "the story's policies");
    assert_eq!(get(campus).residential, Some(ResidentialSpec::Eco), "nothing about its buildings changed");
    assert_eq!(get(strip).story, Some(Story::AfterglowMile));
    assert_eq!(get(odd).story, None);
    assert!(get(odd).is_custom() && get(odd).policies.is_none(), "custom: its dials and no policies");
    assert_eq!(get(odd).story_label(), "Custom");
    assert!(Story::closest(get(odd)).is_some(), "a story is suggested");
    assert_eq!(get(plain).story, None);
    assert_eq!(get(plain).story_label(), "No story");
    assert_eq!(get(plain).height_limit, Some(90.0));
    // Loading again changes nothing.
    let again = World::from_ron(&back.to_ron().unwrap()).unwrap();
    for (a, b) in again.districts.list.iter().zip(&back.districts.list) {
        assert_eq!((a.story, a.policies, a.residential, a.commercial, a.height_limit), (b.story, b.policies, b.residential, b.commercial, b.height_limit));
    }
}

/// [stories] Picking a story sets its building types, height limit and
/// policies; the dials are fine-tuned without leaving the story; setting a
/// building type by hand makes the district custom.
#[test]
fn a_story_sets_the_district_and_dials_fine_tune_it() {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
    let id = w.create_district(None).unwrap();
    w.set_district_policy(id, PolicyChange { story: Some(Some(Story::OldTownCharter)), ..Default::default() }).unwrap();
    let d = w.districts.get(id).unwrap();
    assert!(d.historic && d.height_limit == Some(24.0) && d.policies.heritage_grant > 0.0);
    assert_eq!(d.look_for(LandUse::Residential), Some(crate::districts::Look::Historic));
    w.set_district_policy(id, PolicyChange { policies: vec![(Policy::HeritageGrant, 500.0), (Policy::BedLevy, 0.1)], height_limit: Some(Some(30.0)), ..Default::default() }).unwrap();
    let d = w.districts.get(id).unwrap();
    assert_eq!(d.story, Some(Story::OldTownCharter), "fine-tuning keeps the story");
    assert_eq!(d.policies.heritage_grant, crate::stories::MAX_HERITAGE_GRANT, "clamped");
    assert_eq!(d.policies.bed_levy, 0.1);
    assert_eq!(d.height_limit, Some(30.0));
    w.set_district_policy(id, PolicyChange { story: Some(Some(Story::QuietAcres)), ..Default::default() }).unwrap();
    let d = w.districts.get(id).unwrap();
    // [garden] Quiet Acres builds the Garden Suburb, homes and shops.
    assert!(!d.historic && d.residential == Some(ResidentialSpec::Garden) && d.commercial == Some(CommercialSpec::Garden) && d.policies == Story::QuietAcres.preset().policies);
    w.set_district_policy(id, PolicyChange { commercial: Some(Some(CommercialSpec::NeonStrip)), ..Default::default() }).unwrap();
    assert!(w.districts.get(id).unwrap().is_custom());
    w.set_district_policy(id, PolicyChange { story: Some(None), ..Default::default() }).unwrap();
    let d = w.districts.get(id).unwrap();
    assert!(!d.is_custom() && d.story.is_none() && d.policies.is_none());
}

/// [garden] Saves from before Quiet Acres had its own look: a Quiet Acres
/// district saved with the old Suburban dials takes the Garden Suburb on
/// load (its policies and height kept), and a district from before stories
/// with only Suburban homes still finds Quiet Acres.
#[test]
fn quiet_acres_from_an_old_save_takes_the_garden_suburb() {
    let saved = "(id: 4, name: \"Elm Close\", color: 1, residential: Some(Suburban), commercial: None, industry: None, historic: false, height_limit: Some(15.0), story: Some(QuietAcres), policies: (nightlife_levy: 0.15))";
    let older = "(id: 5, name: \"Oak Rise\", color: 2, residential: Some(Suburban), commercial: None, industry: None, historic: false, height_limit: None)";
    let mut ds = crate::districts::Districts::default();
    ds.list.push(ron::from_str(saved).expect("a story district loads"));
    ds.list.push(ron::from_str(older).expect("an old district loads"));
    assert_eq!(ds.adopt_stories(), 2);
    let (a, b) = (&ds.list[0], &ds.list[1]);
    for d in [a, b] {
        assert_eq!(d.story, Some(Story::QuietAcres), "{}", d.name);
        assert_eq!((d.residential, d.commercial), (Some(ResidentialSpec::Garden), Some(CommercialSpec::Garden)), "{}", d.name);
        assert!(Story::QuietAcres.fits(d));
        assert_eq!(d.look_for(LandUse::Residential), Some(crate::districts::Look::Garden));
        assert_eq!(d.look_for(LandUse::Commercial), Some(crate::districts::Look::Garden));
        assert_eq!(d.look_for(LandUse::Entertainment), None, "nights out keep their own looks");
    }
    assert_eq!(a.policies.nightlife_levy, 0.15, "its own policies kept");
    assert_eq!(a.height_limit, Some(15.0));
    assert_eq!(b.height_limit, None, "an old district keeps its height limit");
    assert_eq!(ds.adopt_stories(), 0, "nothing left to adopt");
}

/// [stories] On a populated town every policy moves money through the
/// ledger's door: levies to the treasury, support, grants and relief from
/// it, all on the district's books and the treasury's district rows, and
/// the books still balance to the unit with nothing moved outside the
/// door. Venues open later; rent stress reads the rent net of support.
#[test]
fn district_policies_move_money_through_the_ledger() {
    let mut w = World::from_ron(&super::legacy_save_text()).expect("the town loads");
    let id = district_over_all(&mut w, "Everywhere");
    w.set_district_policy(
        id,
        PolicyChange {
            story: Some(Some(Story::FoundersCampus)),
            policies: vec![(Policy::RentSupport, 0.3), (Policy::NightlifeLevy, 0.1), (Policy::LateHours, 1.0), (Policy::PollutionLevy, 0.1), (Policy::HeritageGrant, 20.0), (Policy::BusinessRate, -0.03), (Policy::BedLevy, 0.1)],
            ..Default::default()
        },
    )
    .unwrap();
    w.economy.bank.check = true;
    let before = w.economy.bank.untracked_moves;
    let mut minds = IdleMind;
    for _ in 0..(2 * 1440 + 60) {
        w.tick(&mut minds);
    }
    assert_eq!(w.ledger_sum(), 0, "the books balance to the unit");
    assert_eq!(w.reconcile_ledger(), 0, "nothing moved outside the door");
    assert_eq!(w.economy.bank.untracked_moves, before);
    let d = w.districts.get(id).unwrap();
    let y = d.books.yesterday;
    assert!(y[Policy::RentSupport.index()] < 0.0, "rent support paid: {y:?}");
    assert!(y[Policy::HeritageGrant.index()] < 0.0, "heritage grants paid: {y:?}");
    assert!(y[Policy::BusinessRate.index()] < 0.0, "shops got relief: {y:?}");
    assert!(d.books.spent_yesterday() > 0.0);
    let spent = w.economy.ledger.yesterday_of(Flow::DistrictSpending);
    assert!(spent < 0.0, "on the treasury's books as spending: {spent}");
    assert!((spent + d.books.spent_yesterday()).abs() < 1.0, "the district's books and the treasury's agree: {spent} vs {}", d.books.spent_yesterday());
    // Industry and venues pay their levies when the town has any trading.
    let industry_took = w.buildings.values().any(|b| b.business.as_ref().is_some_and(|z| z.kind.industry().is_some() && z.books.last().is_some_and(|k| k.revenue > 0.0)));
    if industry_took {
        assert!(y[Policy::PollutionLevy.index()] > 0.0 || d.books.today[Policy::PollutionLevy.index()] > 0.0, "pollution levy: {y:?}");
    }
    // Late licence on the venues.
    for b in w.buildings.values() {
        if let Some(z) = &b.business {
            let late = World::is_nightlife(z.kind, b.land_use);
            assert_eq!(z.late_hours > 0, late, "{:?}", z.kind);
            if late {
                assert_ne!(b.opening_hours(), z.kind.hours(), "open later");
            }
        }
    }
    // Saved and loaded, the policies and books come back.
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.districts.get(id).unwrap().policies, d.policies);
    assert_eq!(back.districts.get(id).unwrap().books, d.books);
}

/// [stories] Rent support makes a home in the district cheaper to the
/// people choosing one: a household that could not afford it can now.
#[test]
fn rent_support_makes_homes_affordable() {
    let mut w = World::from_ron(&super::legacy_save_text()).expect("the town loads");
    let free = |b: &&Building| b.land_use == LandUse::Residential && b.operating() && b.free_homes() > 0 && b.annex.is_none();
    let Some(home) = w.buildings.iter().filter(|(_, b)| free(b)).map(|(id, _)| id).next() else { panic!("the town has a free home") };
    // Just too dear for this income, everywhere.
    let cheapest = w.buildings.values().filter(free).map(|b| b.rent).fold(f32::INFINITY, f32::min);
    let income = cheapest / super::trips::RENT_STRESS_SHARE * 0.8;
    assert!(w.choose_home(income, None, false).is_none());
    let id = district_over_all(&mut w, "Everywhere");
    w.set_district_policy(id, PolicyChange { policies: vec![(Policy::RentSupport, 0.5)], ..Default::default() }).unwrap();
    assert!(w.choose_home(income, None, false).is_some(), "affordable with half the rent paid");
    assert!(w.district_pull(w.buildings[home].lot, LandUse::Residential) > 1.0, "and more homes go up");
}

/// [stories] Nothing changes in a city whose districts set no policy: the
/// same run with and without a policy-free district is identical.
#[test]
fn no_policy_no_change() {
    let run = |district: bool| {
        let mut w = World::from_ron(&super::legacy_save_text()).expect("the town loads");
        if district {
            let id = district_over_all(&mut w, "Plain");
            w.set_district_policy(id, PolicyChange { story: Some(None), ..Default::default() }).unwrap();
        }
        let mut minds = IdleMind;
        for _ in 0..(1440 + 30) {
            w.tick(&mut minds);
        }
        (w.economy.treasury.to_bits(), w.citizens.len(), w.households.values().map(|h| (h.money * 10.0) as i64).sum::<i64>())
    };
    assert_eq!(run(false), run(true));
}
