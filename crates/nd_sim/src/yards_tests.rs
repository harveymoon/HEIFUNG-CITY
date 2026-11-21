//! [r12-yards] What a household puts out follows the household: children
//! bring bikes and toys, the old pots and a chair, the washing follows the
//! weather, trades bring their things home; the same household always
//! shows the same, and a changed one shows something else.

use super::*;
use crate::citizens::Household;
use crate::weather::Weather;

fn legacy() -> World {
    World::from_ron(&crate::world::legacy_save_text()).expect("loads")
}

/// A house (one home, two floors) given a fresh household of these ages.
fn home_with(w: &mut World, ages: &[u8]) -> (BuildingId, HouseholdId, Vec<CitizenId>) {
    let bid = w.buildings.iter().find(|(_, b)| b.homes > 0 && b.construction.is_none() && b.demolition.is_none()).map(|(id, _)| id).expect("a home");
    let template = w.citizens.values().next().expect("a citizen").clone();
    let hid = w.households.insert(Household { home: Some(bid), ..Default::default() });
    let mut ids = Vec::new();
    for (i, &age) in ages.iter().enumerate() {
        let mut c = template.clone();
        c.name = format!("Person{i} Testerly");
        c.seed = 0x5eed_0000 + i as u64 * 7919;
        c.age = age;
        c.owns_bike = false;
        c.vehicle = None;
        c.work = None;
        c.household = Some(hid);
        c.home = Some(bid);
        ids.push(w.citizens.insert(c));
    }
    w.households[hid].members = ids.clone();
    let b = &mut w.buildings[bid];
    b.households = vec![hid];
    b.homes = 1;
    b.floors = 2;
    (bid, hid, ids)
}

fn kinds_of(ws: &[Wish], owner: CitizenId) -> Vec<PropKind> {
    ws.iter().filter(|w| w.owner == owner).map(|w| w.kind).collect()
}

#[test]
fn children_bring_bikes_and_toys_the_old_pots_and_a_chair() {
    let mut w = legacy();
    let (bid, _, ids) = home_with(&mut w, &[38, 8, 3]);
    let ws = home_wishes(&w, bid, false, HomeCtx::default());
    let eight = kinds_of(&ws, ids[1]);
    assert!(eight.contains(&PropKind::KidBike), "an eight-year-old's bike: {ws:?}");
    assert!(eight.iter().any(|k| matches!(k, PropKind::Ball | PropKind::Chalk)), "and a ball or chalk: {eight:?}");
    let three = kinds_of(&ws, ids[2]);
    assert!(three.iter().any(|k| matches!(k, PropKind::Trike | PropKind::Toys)), "a toddler's trike or toys: {three:?}");
    assert!(!ws.iter().any(|w| w.kind == PropKind::Chair), "no old folk, no chair out front");
    assert!(ws.iter().filter(|w| matches!(w.kind, PropKind::KidBike | PropKind::Trike | PropKind::Toys)).all(|w| w.place == Place::Front));

    let mut w = legacy();
    let (bid, _, ids) = home_with(&mut w, &[78, 74]);
    let ws = home_wishes(&w, bid, false, HomeCtx::default());
    assert!(ws.iter().any(|w| w.kind == PropKind::Plant && w.place == Place::Step && ids.contains(&w.owner)), "pots on the step: {ws:?}");
    assert!(ws.iter().any(|w| w.kind == PropKind::Chair && w.owner == ids[0]), "the eldest's chair: {ws:?}");
    assert!(!ws.iter().any(|w| matches!(w.kind, PropKind::KidBike | PropKind::Trike | PropKind::Toys | PropKind::Chalk)), "no toys without children");
    assert_eq!(line(&w, ws.iter().find(|w| w.kind == PropKind::Chair).unwrap()), "Person0's chair out front");
}

#[test]
fn the_washing_follows_the_weather() {
    let sunny = Weather { cloud: 0.15, wind: 0.4, climate: Climate::Temperate, ..Weather::default() };
    let rain = Weather { cloud: 0.8, ..sunny.clone() };
    let storm = Weather { spell: crate::weather::Spell::Storm, cloud: 0.3, ..sunny.clone() };
    let grey = Weather { cloud: 0.58, ..sunny.clone() };
    assert!(laundry_out(&sunny, 11));
    assert!(!laundry_out(&sunny, 22), "in at night");
    assert!(!laundry_out(&sunny, 6), "not yet at dawn");
    assert!(!laundry_out(&rain, 11) && wet(&rain), "in when it rains");
    assert!(!laundry_out(&storm, 11), "in in a storm");
    assert!(!laundry_out(&grey, 11) && !wet(&grey), "a grey day: dry but not worth it");
    // The desert's heavy cloud brings no rain: dry enough, but not bright.
    let desert = Weather { climate: Climate::Desert, cloud: 0.7, ..sunny.clone() };
    assert!(!wet(&desert));

    // Out on a sunny day for most families, in for all of them in the rain.
    let mut w = legacy();
    let (bid, _, _) = home_with(&mut w, &[40, 41, 10]);
    let hid = w.buildings[bid].households[0];
    let mut out = 0;
    for k in 0..24u64 {
        // Different families in the same house.
        for (i, m) in w.households[hid].members.clone().into_iter().enumerate() {
            w.citizens[m].seed = 0xa11 + k * 101 + i as u64;
        }
        let dry = home_wishes(&w, bid, laundry_out(&sunny, 11), HomeCtx::default());
        let wet_day = home_wishes(&w, bid, laundry_out(&rain, 11), HomeCtx::default());
        out += dry.iter().any(|w| w.kind == PropKind::Laundry) as usize;
        assert!(!wet_day.iter().any(|w| w.kind == PropKind::Laundry), "in when it rains");
        // The rest stays put whatever the weather.
        let dry_rest: Vec<_> = dry.iter().filter(|w| w.kind != PropKind::Laundry).collect();
        let wet_rest: Vec<_> = wet_day.iter().filter(|w| w.kind != PropKind::Laundry).collect();
        if dry_rest.len() + 1 >= dry.len() && dry.len() < MAX_PER_HOUSEHOLD {
            assert_eq!(dry_rest, wet_rest);
        }
    }
    assert!(out >= 12, "most families hang it out on a sunny day: {out} of 24");
}

#[test]
fn trades_and_leanings_bring_their_things_home() {
    let mut w = legacy();
    let (bid, _, ids) = home_with(&mut w, &[33]);
    let (job, _) = w.buildings.iter().find(|(_, b)| b.business.is_some()).map(|(id, b)| (id, b.lot)).expect("a business");
    w.buildings[job].business.as_mut().unwrap().kind = BusinessKind::Mechanic;
    w.citizens[ids[0]].work = Some(job);
    let ws = home_wishes(&w, bid, false, HomeCtx::default());
    assert!(kinds_of(&ws, ids[0]).contains(&PropKind::ProjectBike), "a mechanic's motorbike project: {ws:?}");
    w.buildings[job].business.as_mut().unwrap().kind = BusinessKind::Diner;
    let ws = home_wishes(&w, bid, false, HomeCtx::default());
    assert!(kinds_of(&ws, ids[0]).contains(&PropKind::Grill), "a cook's grill: {ws:?}");
    w.buildings[job].business.as_mut().unwrap().kind = BusinessKind::BeachBar;
    assert!(!home_wishes(&w, bid, false, HomeCtx::default()).iter().any(|w| w.kind == PropKind::Surfboard), "no surfboard inland");
    let ws = home_wishes(&w, bid, false, HomeCtx { near_beach: true });
    assert!(kinds_of(&ws, ids[0]).contains(&PropKind::Surfboard), "a beach bar hand's surfboard: {ws:?}");
    // A bike of their own.
    w.citizens[ids[0]].owns_bike = true;
    let any_bike = (0..16u64).any(|k| {
        w.citizens[ids[0]].seed = 0xb1c + k;
        kinds_of(&home_wishes(&w, bid, false, HomeCtx::default()), ids[0]).contains(&PropKind::Bike)
    });
    assert!(any_bike);
}

#[test]
fn a_block_puts_everything_at_its_windows() {
    let mut w = legacy();
    let (bid, _, _) = home_with(&mut w, &[80, 36, 9]);
    w.buildings[bid].homes = 24;
    w.buildings[bid].floors = 6;
    let ws = home_wishes(&w, bid, true, HomeCtx::default());
    assert!(!ws.is_empty());
    assert!(ws.iter().all(|w| w.place == Place::Window), "{ws:?}");
    assert!(!ws.iter().any(|w| matches!(w.kind, PropKind::Chair | PropKind::Grill | PropKind::ProjectBike | PropKind::Tools)), "no yard to keep them in: {ws:?}");
    assert!(ws.iter().any(|w| w.kind == PropKind::Plant), "the old lady's window box");
}

#[test]
fn the_same_household_shows_the_same_and_a_changed_one_shows_more() {
    let mut w = legacy();
    let (bid, hid, ids) = home_with(&mut w, &[36, 7]);
    let a = home_wishes(&w, bid, true, HomeCtx::default());
    let b = home_wishes(&w, bid, true, HomeCtx::default());
    assert_eq!(a, b);
    // A copy of the city shows the same (nothing is drawn from a stream).
    let copy = w.clone();
    assert_eq!(home_wishes(&copy, bid, true, HomeCtx::default()), a);
    // The member order in the household does not matter.
    w.households[hid].members.reverse();
    assert_eq!(home_wishes(&w, bid, true, HomeCtx::default()).len(), a.len());
    let sig = home_signature(&w, bid);
    // The child grows up: the little bike goes.
    w.citizens[ids[1]].age = 19;
    assert_ne!(home_signature(&w, bid), sig, "the signature sees it");
    let grown = home_wishes(&w, bid, true, HomeCtx::default());
    assert!(a.iter().any(|w| w.kind == PropKind::KidBike));
    assert!(!grown.iter().any(|w| w.kind == PropKind::KidBike), "{grown:?}");
    // A baby arrives: a trike or toys.
    let baby = {
        let mut c = w.citizens[ids[0]].clone();
        c.seed = 0xbab1;
        c.age = 2;
        c.name = "Mika Testerly".into();
        w.citizens.insert(c)
    };
    w.households[hid].members.push(baby);
    let more = home_wishes(&w, bid, true, HomeCtx::default());
    assert!(more.iter().any(|w| w.owner == baby && matches!(w.kind, PropKind::Trike | PropKind::Toys)), "{more:?}");
    // Nobody home, nothing out.
    w.buildings[bid].households.clear();
    assert!(home_wishes(&w, bid, true, HomeCtx::default()).is_empty());
}
