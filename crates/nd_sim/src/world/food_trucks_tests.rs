//! [r14-foodtrucks] Tests of food trucks: where they choose to go, that
//! they park legally and go home, and that a meal from one feeds people.

use super::*;
use crate::food_trucks::*;
use crate::mind::IdleMind;
use crate::vehicle::Errand;
use std::collections::BTreeMap;

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

fn finish_all(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

fn spot(kind: SpotKind, x: f32, crowd: f32, from: u8, to: u8) -> CrowdSpot {
    CrowdSpot { kind, at: Vec2::new(x, 0.0), name: format!("{kind:?} at {x}"), crowd, from, to, building: None }
}

fn warm(_: u8) -> f32 {
    24.0
}

fn snow(_: u8) -> f32 {
    -3.0
}

/// The crowds decide: a busy park beats a quiet one; a taco truck goes to
/// the stadium on a game night (before kick-off and after the whistle);
/// a coffee cart to the offices in the morning.
#[test]
fn trucks_go_where_the_crowds_are() {
    let none = BTreeMap::new();
    let spots = vec![spot(SpotKind::Park, 0.0, 15.0, 10, 20), spot(SpotKind::Park, 300.0, 120.0, 10, 20)];
    let picks = pick_stints(TruckKind::IceCream, Vec2::ZERO, 6, &spots, &none, &warm, false, 1, &[], MAX_STINTS);
    assert_eq!(picks.first().map(|p| p.0), Some(1), "the busy park first: {picks:?}");
    let spots = vec![spot(SpotKind::Park, 0.0, 30.0, 10, 20), spot(SpotKind::Stadium, 500.0, 320.0, 16, 23)];
    let picks = pick_stints(TruckKind::Taco, Vec2::ZERO, 6, &spots, &none, &warm, false, 2, &[], MAX_STINTS);
    let stadium = picks.iter().find(|p| p.0 == 1).expect("the taco truck works the game");
    assert!(stadium.1 >= 16 && stadium.2 <= 23, "in the game's hours: {stadium:?}");
    let spots = vec![spot(SpotKind::Offices, 0.0, 80.0, 7, 15), spot(SpotKind::Nightlife, 200.0, 80.0, 20, 27)];
    let picks = pick_stints(TruckKind::Coffee, Vec2::ZERO, 6, &spots, &none, &warm, false, 3, &[], MAX_STINTS);
    assert_eq!(picks.len(), 1, "a coffee cart never works the bars: {picks:?}");
    assert_eq!(picks[0].0, 0);
    assert!(picks[0].1 < 12, "coffee in the morning: {picks:?}");
    let picks = pick_stints(TruckKind::Kebab, Vec2::ZERO, 6, &spots, &none, &warm, false, 4, &[], MAX_STINTS);
    assert_eq!(picks.first().map(|p| p.0), Some(1), "the kebab van works the bars late: {picks:?}");
    // Two stints a day at most, an hour apart, never overlapping.
    for w in picks.windows(2) {
        assert!(w[1].1 > w[0].2, "{picks:?}");
    }
}

/// Snow (or rain) keeps the ice-cream van at home; a warm dry day sends it
/// to the park and the beach. A hot coffee does better in the cold.
#[test]
fn ice_cream_follows_the_weather() {
    let none = BTreeMap::new();
    let spots = vec![spot(SpotKind::Park, 0.0, 80.0, 10, 20), spot(SpotKind::Beach, 400.0, 60.0, 10, 19)];
    assert!(pick_stints(TruckKind::IceCream, Vec2::ZERO, 6, &spots, &none, &snow, false, 5, &[], MAX_STINTS).is_empty(), "no ice cream in the snow");
    assert!(pick_stints(TruckKind::IceCream, Vec2::ZERO, 6, &spots, &none, &warm, true, 5, &[], MAX_STINTS).is_empty(), "nor in the rain");
    assert!(!pick_stints(TruckKind::IceCream, Vec2::ZERO, 6, &spots, &none, &warm, false, 5, &[], MAX_STINTS).is_empty(), "a sunny day: out to the park or the beach");
    assert!(!ice_cream_weather(5.0, false) && ice_cream_weather(22.0, false) && !ice_cream_weather(22.0, true));
    let office = spot(SpotKind::Offices, 0.0, 50.0, 7, 15);
    assert!(appeal(TruckKind::Coffee, &office, 8, -2.0, false) > appeal(TruckKind::Coffee, &office, 8, 18.0, false));
}

/// Trucks spread out: with one truck already going to the big spot, the
/// next goes to the other when it is nearly as good.
#[test]
fn trucks_spread_over_the_spots() {
    let spots = vec![spot(SpotKind::Park, 0.0, 100.0, 10, 20), spot(SpotKind::Park, 100.0, 80.0, 10, 20)];
    let none = BTreeMap::new();
    let first = pick_stints(TruckKind::HotDog, Vec2::ZERO, 6, &spots, &none, &warm, false, 6, &[], 1);
    assert_eq!(first[0].0, 0);
    let taken: BTreeMap<usize, u32> = [(0usize, 1u32)].into();
    let second = pick_stints(TruckKind::HotDog, Vec2::ZERO, 6, &spots, &taken, &warm, false, 6, &[], 1);
    assert_eq!(second[0].0, 1, "the second truck takes the other park");
}

/// A main street with shops at the west end and homes at the east end.
fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], 1, 1);
    w.add_zone(square(-650.0, 10.0, 180.0), LandUse::Commercial);
    w.add_zone(square(470.0, 10.0, 180.0), LandUse::Residential);
    run(&mut w, 10 * 60);
    finish_all(&mut w);
    run(&mut w, 60);
    w
}

/// A household with savings and a grown-up at home with no job.
fn owner(w: &mut World) -> (HouseholdId, CitizenId) {
    let id = w.citizens.iter().find(|(_, c)| c.home.is_some() && c.can_work() && c.household.is_some()).map(|(id, _)| id).expect("someone moved in");
    let home = w.citizens[id].home.unwrap();
    let hh = w.citizens[id].household.unwrap();
    // Everyone else in the household is out of the way.
    for m in w.households[hh].members.clone() {
        if m != id {
            if let Some(c) = w.citizens.get_mut(m) {
                c.next_decision_tick = u64::MAX;
            }
        }
    }
    w.households[hh].money = 3000.0;
    let c = &mut w.citizens[id];
    c.work = None;
    c.activity = Activity::AtHome;
    c.inside = Some(home);
    c.asleep = false;
    c.next_decision_tick = u64::MAX;
    (hh, id)
}

/// A truck drives out in the traffic, parks in a legal kerb slot near its
/// spot (holding it: no car takes it), opens, sells a meal that feeds a
/// hungry customer and pays the household, closes at the end of the stint
/// and drives home, its cook back indoors.
#[test]
fn a_truck_parks_legally_sells_meals_and_goes_home() {
    let mut w = town();
    let (hh, cook) = owner(&mut w);
    let id = w.found_food_truck(TruckKind::Taco, Some(hh)).expect("a truck starts up");
    let hour = w.clock.hour() as u8;
    let day = w.clock.day();
    // Along the homes' stretch of the main street, where cars may park.
    let at = Vec2::new(250.0, 20.0);
    w.food_trucks.trucks[id].plan = vec![Stint { kind: SpotKind::Offices, at, name: "the test offices".into(), from: hour + 1, to: hour + 3, crowd: 50.0, state: StintState::Planned, day }];
    let money0 = w.households[hh].money;
    // Out it goes, as a vehicle on the road.
    let mut drove = false;
    let mut minds = IdleMind;
    for _ in 0..200 {
        w.tick(&mut minds);
        if let TruckPlace::Road(car) = w.food_trucks.trucks[id].place {
            drove = true;
            assert_eq!(w.traffic.cars[car].errand, Errand::FoodTruck(id));
            assert!(matches!(w.citizens[cook].activity, Activity::Riding { bus } if bus == car), "the cook rides along");
        }
        if w.food_trucks.trucks[id].place.open() {
            break;
        }
    }
    assert!(drove, "it drove out: {:?}", w.food_trucks.trucks[id].plan);
    assert!(w.food_trucks.trucks[id].place.open(), "open at its pitch: {:?} {:?}", w.food_trucks.trucks[id].place, w.food_trucks.trucks[id].plan);
    assert!(w.food_trucks.trucks[id].stock > 10.0, "it bought its ingredients");
    assert!(w.households[hh].money < money0, "paid for them");
    let pitch = *w.food_trucks.trucks[id].place.pitch().unwrap();
    assert!(pitch.pos.distance(at) <= PARK_REACH, "parked near the spot");
    assert!(pitch.legal && w.kerb_allowed(pitch.segment), "in a legal kerb slot");
    let (kp, kh) = w.slot_pose(pitch.segment, pitch.side, pitch.slot);
    assert!(kp.distance(pitch.pos) < 0.5 && (kh - pitch.heading).abs() < 1e-3, "standing in the slot");
    assert!(!w.free_kerbs_near(pitch.pos, 3.0, 4).iter().any(|k| k.segment == pitch.segment && k.side == pitch.side && k.slot == pitch.slot), "no car may take its slot");
    assert!(w.citizens[cook].pos.distance(pitch.pos) < 3.0, "the cook at the hatch");
    // A hungry passer-by.
    let eater = w.citizens.iter().find(|(cid, c)| *cid != cook && c.home.is_some() && c.age >= 18 && c.household != Some(hh)).map(|(id, _)| id).expect("a neighbour");
    {
        let c = &mut w.citizens[eater];
        c.satiety = 0.1;
        c.activity = Activity::Idle;
        c.inside = None;
        c.pos = pitch.pos + pitch.out() * 6.0 + pitch.fwd() * 30.0;
        c.prev_pos = c.pos;
        c.savings = 100.0;
    }
    if let Some(h) = w.citizens[eater].household {
        w.households[h].money = 200.0;
    }
    assert!(w.truck_bite(eater, BITE_REACH, 1.0), "a hungry neighbour goes for a bite");
    let fed0 = w.citizens[eater].satiety;
    let owner_money = w.households[hh].money;
    for _ in 0..60 {
        w.tick(&mut minds);
        if w.food_trucks.trucks[id].meals_today > 0 {
            break;
        }
    }
    assert_eq!(w.food_trucks.trucks[id].meals_today, 1, "one meal sold");
    assert!(w.citizens[eater].satiety > fed0 + 0.3, "the meal fed them: {} -> {}", fed0, w.citizens[eater].satiety);
    assert!(w.households[hh].money > owner_money, "the takings went to the household");
    assert_eq!(w.food_trucks.stats.fed_hungry_today, 1);
    // The stint ends: packed up and home.
    for _ in 0..240 {
        w.tick(&mut minds);
        if w.food_trucks.trucks[id].place == TruckPlace::Base {
            break;
        }
    }
    assert_eq!(w.food_trucks.trucks[id].place, TruckPlace::Base, "home again: {:?}", w.food_trucks.trucks[id].plan);
    assert_eq!(w.food_trucks.trucks[id].crew, None);
    assert!(matches!(w.citizens[cook].activity, Activity::AtHome), "the cook indoors: {:?}", w.citizens[cook].activity);
    assert!(matches!(w.food_trucks.trucks[id].plan[0].state, StintState::Done));
    assert_eq!(w.food_trucks.trucks[id].visits.len(), 1);
    // Saved and loaded, it is all still there.
    let ron = ron::to_string(&w.food_trucks).unwrap();
    let back: FoodTrucks = ron::from_str(&ron).unwrap();
    assert_eq!(back.trucks.len(), 1);
}

/// An ice-cream van with a stint in the snow stays at home.
#[test]
fn an_ice_cream_van_stays_home_in_the_snow() {
    let mut w = town();
    let (hh, _) = owner(&mut w);
    let id = w.found_food_truck(TruckKind::IceCream, Some(hh)).expect("a van starts up");
    w.weather.temp_c = -4.0;
    let hour = w.clock.hour() as u8;
    let day = w.clock.day();
    w.food_trucks.trucks[id].plan = vec![Stint { kind: SpotKind::Park, at: Vec2::new(-250.0, 30.0), name: "the park".into(), from: hour + 1, to: hour + 4, crowd: 50.0, state: StintState::Planned, day }];
    let mut minds = IdleMind;
    for _ in 0..90 {
        w.weather.temp_c = -4.0;
        w.tick(&mut minds);
    }
    assert_eq!(w.food_trucks.trucks[id].place, TruckPlace::Base);
    assert!(matches!(w.food_trucks.trucks[id].plan[0].state, StintState::Skipped(_)), "{:?}", w.food_trucks.trucks[id].plan);
    assert_eq!(w.food_trucks.stats.too_cold_today, 1);
}
