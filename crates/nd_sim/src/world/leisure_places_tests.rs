//! [r4-leisure-places] Places to eat and go out: each kind opens in the
//! right sub-zone, people visit them at the right hours, stock and money
//! flow through them, and saves (old ones too) keep them.

use super::*;
use crate::commercial::CommercialSub;
use crate::leisure::{EATERIES, NIGHTS_OUT};
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

fn vacant(w: &World, poly: &[Vec2], lu: LandUse) -> Vec<LotId> {
    let mut v: Vec<(LotId, Vec2)> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == lu && point_in_polygon(l.center, poly)).map(|(id, l)| (id, l.center)).collect();
    v.sort_by(|a, b| a.1.x.total_cmp(&b.1.x).then(a.1.y.total_cmp(&b.1.y)));
    v.into_iter().map(|(id, _)| id).collect()
}

fn kind_of(w: &World, b: BuildingId) -> Option<BusinessKind> {
    w.buildings.get(b).and_then(|x| x.business.as_ref()).map(|z| z.kind)
}

/// A town: a long street, homes to the south, commercial and leisure land
/// to the north, and people in the homes.
fn town() -> (World, Vec<Vec2>, Vec<Vec2>) {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-500.0, 0.0), Vec2::new(500.0, 0.0)], 1, 1);
    let shops = square(-480.0, 10.0, 470.0);
    let leisure = square(10.0, 10.0, 470.0);
    w.add_zone(shops.clone(), LandUse::Commercial);
    w.add_zone(leisure.clone(), LandUse::Entertainment);
    w.add_zone(square(-480.0, -480.0, 960.0).into_iter().map(|p| Vec2::new(p.x, p.y.min(-10.0))).collect(), LandUse::Residential);
    (w, shops, leisure)
}

#[test]
fn every_kind_has_its_sub_zone_and_neighbourhood_and_retail_differ() {
    use BusinessKind as K;
    let n = CommercialSub::Neighbourhood.menu();
    let r = CommercialSub::Retail.menu();
    let l = CommercialSub::Leisure.menu();
    for k in [K::Diner, K::NoodleBar, K::PizzaPlace, K::BubbleTea] {
        assert!(n.contains(&k), "{k:?} is a neighbourhood place");
    }
    for k in [K::FastFood, K::FoodHall, K::Gym] {
        assert!(r.contains(&k) && !n.contains(&k), "{k:?} is retail's");
    }
    for k in [K::Venue, K::Bar, K::KaraokeBox, K::Cinema, K::Theatre, K::ConcertHall, K::BowlingAlley, K::Arcade, K::ComedyClub, K::FineDining, K::StreetFood] {
        assert!(l.contains(&k), "{k:?} grows on leisure land");
    }
    // Every new kind grows somewhere by the market.
    for k in EATERIES.into_iter().chain(NIGHTS_OUT) {
        assert!(CommercialSub::ALL.iter().any(|s| s.menu().contains(&k)), "{k:?}");
        assert!(crate::tax::TaxCategory::of_business(k, Tier::Mid).is_some(), "{k:?} pays a tax line");
    }
    // Neighbourhood and retail are no longer the same shops.
    let only_n: Vec<_> = n.iter().filter(|k| !r.contains(k)).collect();
    let only_r: Vec<_> = r.iter().filter(|k| !n.contains(k)).collect();
    assert!(only_n.len() >= 3 && only_r.len() >= 3, "{only_n:?} / {only_r:?}");
    // The nights out are taxed on the nightlife lines, the kitchens with the cafés.
    assert_eq!(crate::tax::TaxCategory::of_business(K::Bar, Tier::Mid), Some(crate::tax::TaxCategory::NightlifeMid));
    assert_eq!(crate::tax::TaxCategory::of_business(K::Theatre, Tier::High), Some(crate::tax::TaxCategory::NightlifeHigh));
    assert_eq!(crate::tax::TaxCategory::of_business(K::Diner, Tier::Low), Some(crate::tax::TaxCategory::Bakery));
}

#[test]
fn each_sub_zone_opens_its_own_kinds() {
    let (mut w, shops, leisure) = town();
    // A town big enough for its places to eat and go out.
    w.economy.population = 5000;
    let lots = vacant(&w, &shops, LandUse::Commercial);
    assert!(lots.len() >= 16, "{} lots", lots.len());
    let (west, east) = lots.split_at(lots.len() / 2);
    for l in west {
        w.set_commercial_bias(w.lots[*l].polygon.clone(), Some(CommercialSub::Neighbourhood)).unwrap();
    }
    for l in east {
        w.set_commercial_bias(w.lots[*l].polygon.clone(), Some(CommercialSub::Retail)).unwrap();
    }
    let mut seen = Vec::new();
    for (sub, set) in [(CommercialSub::Neighbourhood, west), (CommercialSub::Retail, east)] {
        for l in set.iter().take(10) {
            let Some(b) = w.build_on_as(*l, None) else { continue };
            finish_all(&mut w);
            let k = kind_of(&w, b).expect("a business");
            assert!(sub.menu().contains(&k), "{sub:?} opened a {k:?}");
            seen.push((sub, k));
        }
    }
    assert!(seen.iter().any(|(s, k)| *s == CommercialSub::Neighbourhood && k.is_eatery()), "a corner place to eat: {seen:?}");
    assert!(seen.iter().any(|(s, k)| *s == CommercialSub::Retail && matches!(k, BusinessKind::FastFood | BusinessKind::FoodHall | BusinessKind::Gym)), "a retail kind of its own: {seen:?}");
    // Leisure land: the club first, then the rest of the nights out.
    let llots = vacant(&w, &leisure, LandUse::Entertainment);
    let mut kinds = Vec::new();
    for l in llots.iter().take(10) {
        if let Some(b) = w.build_on_as(*l, None) {
            finish_all(&mut w);
            kinds.push(kind_of(&w, b).unwrap());
        }
    }
    assert_eq!(kinds.first(), Some(&BusinessKind::Venue), "{kinds:?}");
    assert!(kinds.iter().all(|k| CommercialSub::Leisure.menu().contains(k)), "{kinds:?}");
    assert!(kinds.iter().filter(|k| k.is_eat_or_go_out()).count() >= 3, "{kinds:?}");
    // Each is named for its trade, never a club's name.
    for (_, b) in w.buildings.iter().filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind.is_eat_or_go_out())) {
        let k = b.business.as_ref().unwrap().kind;
        assert!(!b.name.ends_with(" Club") || matches!(k, BusinessKind::ComedyClub | BusinessKind::Gym), "{}", b.name);
    }
}

#[test]
fn on_auto_quiet_streets_and_avenues_grow_different_places() {
    use crate::commercial::AutoSite;
    let quiet = AutoSite { lanes: 2, homes_near: 150.0, land_value: 0.4, shop_demand: 0.5, leisure_demand: 0.3, nightlife: false, population: 3000, unemployment: 0.03, seed: 1 };
    let avenue = AutoSite { lanes: 4, homes_near: 10.0, ..quiet };
    let all = [true; 4];
    let (a, b) = (quiet.pick(all).unwrap(), avenue.pick(all).unwrap());
    assert_eq!((a, b), (CommercialSub::Neighbourhood, CommercialSub::Retail));
    let na: Vec<_> = a.auto_menu().iter().filter(|k| k.is_eatery()).collect();
    let nb: Vec<_> = b.auto_menu().iter().filter(|k| k.is_eatery()).collect();
    assert!(na.iter().any(|k| **k == BusinessKind::Diner) && !nb.iter().any(|k| **k == BusinessKind::Diner));
    assert!(nb.iter().any(|k| **k == BusinessKind::FastFood) && !na.iter().any(|k| **k == BusinessKind::FastFood));
}

/// A town with one of each of `kinds` open on its leisure and commercial
/// land, and people moved in.
fn open_places(kinds: &[BusinessKind]) -> (World, Vec<BuildingId>) {
    let (mut w, shops, leisure) = town();
    // Homes first, then let people arrive.
    for l in w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential && l.building.is_none()).map(|(id, _)| id).take(12).collect::<Vec<_>>() {
        w.build_on(l);
    }
    let mut places = Vec::new();
    let mut c_lots = vacant(&w, &shops, LandUse::Commercial).into_iter();
    let mut l_lots = vacant(&w, &leisure, LandUse::Entertainment).into_iter();
    for &k in kinds {
        let lot = if crate::commercial::CommercialSub::Leisure.menu().contains(&k) { l_lots.next() } else { c_lots.next() };
        let b = w.build_on_as(lot.expect("a lot"), Some(k)).expect("built");
        places.push(b);
    }
    finish_all(&mut w);
    let mut minds = IdleMind;
    for _ in 0..1200 {
        if w.citizens.len() >= 20 {
            break;
        }
        w.tick(&mut minds);
    }
    finish_all(&mut w);
    assert!(w.citizens.len() >= 5, "people moved in: {}", w.citizens.len());
    for &b in &places {
        assert!(w.buildings[b].operating(), "{:?} open", kind_of(&w, b));
    }
    (w, places)
}

/// Whom `choose_place` picks for `cid` at `hour` over a few draws.
fn picks(w: &mut World, cid: CitizenId, hour: u64) -> Vec<Option<BusinessKind>> {
    let mut out = Vec::new();
    for k in 0..12u64 {
        w.clock = crate::clock::SimClock::starting_at(3 + k, hour, 5);
        w.crowds_cache = None;
        let b = w.choose_place(cid, LandUse::Entertainment, false);
        out.push(b.and_then(|b| kind_of(w, b)));
    }
    out
}

#[test]
fn people_go_out_at_the_right_hours() {
    use BusinessKind as K;
    let (mut w, _) = open_places(&[K::Gym, K::Cinema, K::Diner, K::Venue]);
    // Gym-goers and the rest, by their seed; all adults with money, fed.
    let adults: Vec<CitizenId> = w.citizens.iter().filter(|(_, c)| c.stage() == crate::citizens::LifeStage::Adult).map(|(id, _)| id).collect();
    assert!(!adults.is_empty());
    for &c in &adults {
        let x = &mut w.citizens[c];
        x.savings = 400.0;
        x.satiety = 0.9;
        x.places = Default::default();
    }
    let goer = |w: &World, c: CitizenId| w.outgoer(c).is_some_and(|o| o.gym_goer);
    let count = |v: &[Option<K>], k: K| v.iter().filter(|x| **x == Some(k)).count();
    // Seven in the morning: the gym's regulars at the gym.
    let mut gym_am = (0, 0);
    let mut cinema_pm = (0, 0);
    let mut diner_noon = (0, 0);
    for &c in &adults {
        if goer(&w, c) {
            let v = picks(&mut w, c, 7);
            gym_am.0 += count(&v, K::Gym);
            gym_am.1 += v.len();
        } else {
            let v = picks(&mut w, c, 20);
            cinema_pm.0 += count(&v, K::Cinema);
            cinema_pm.1 += count(&v, K::Gym);
            w.citizens[c].satiety = 0.2;
            let v = picks(&mut w, c, 12);
            diner_noon.0 += count(&v, K::Diner);
            diner_noon.1 += v.len();
            w.citizens[c].satiety = 0.9;
        }
    }
    if gym_am.1 > 0 {
        assert!(gym_am.0 * 2 > gym_am.1, "gym-goers at the gym in the morning: {gym_am:?}");
    }
    assert!(cinema_pm.0 > cinema_pm.1 * 3, "the evening at the cinema, not the gym: {cinema_pm:?}");
    assert!(diner_noon.0 * 2 > diner_noon.1, "hungry at noon: the diner ({diner_noon:?})");
}

#[test]
fn a_gym_goer_heads_out_in_the_morning_and_families_bowl_together() {
    use BusinessKind as K;
    let (mut w, places) = open_places(&[K::Gym, K::BowlingAlley, K::Venue]);
    w.economy.market.leisure = Default::default();
    // A visit to each counts by kind and hour.
    for (&b, hour) in places.iter().zip([7u64, 18, 23]) {
        let c = w.citizens.keys().next().unwrap();
        w.clock = crate::clock::SimClock::starting_at(4, hour, 0);
        w.citizens[c].savings = 200.0;
        let entrance = w.building_entrance(b).unwrap();
        w.citizens[c].activity = Activity::Traveling { to: Some(b), purpose: Purpose::Visit(LandUse::Entertainment), path: vec![entrance], idx: 1 };
        w.arrive(c);
        assert_eq!(w.citizens[c].inside, Some(b));
    }
    let s = &w.economy.market.leisure;
    assert_eq!(s.by_hour(K::Gym, false)[7], 1);
    assert_eq!(s.by_hour(K::BowlingAlley, false)[18], 1);
    assert_eq!(s.visits(K::Venue, false), 1);
    assert!(w.citizens.values().any(|c| c.memories.iter().any(|m| m.text.starts_with("Worked out at"))));
    // Rolled at midnight.
    w.economy.market.leisure.roll();
    assert_eq!(w.economy.market.leisure.visits(K::Gym, true), 1);
    assert_eq!(w.economy.market.leisure.visits(K::Gym, false), 0);
}

#[test]
fn stock_and_money_flow_through_the_places() {
    use BusinessKind as K;
    let (mut w, places) = open_places(&[K::Bar, K::Cinema, K::Diner]);
    let (bar, cinema, diner) = (places[0], places[1], places[2]);
    let sum0 = w.ledger_sum();
    w.buildings[bar].business.as_mut().unwrap().add(Resource::Groceries, 20.0);
    w.buildings[cinema].business.as_mut().unwrap().add(Resource::Groceries, 20.0);
    w.buildings[diner].business.as_mut().unwrap().add(Resource::BakedGoods, 10.0);
    let people: Vec<CitizenId> = w.citizens.keys().take(3).collect();
    assert_eq!(people.len(), 3);
    w.clock = crate::clock::SimClock::starting_at(4, 20, 0);
    for (&c, &b) in people.iter().zip(&[bar, cinema, diner]) {
        w.citizens[c].savings = 300.0;
        w.citizens[c].satiety = 0.3;
        w.citizens[c].activity = Activity::Visiting(b);
        w.citizens[c].inside = Some(b);
    }
    let rev = |w: &World, b: BuildingId| w.buildings[b].business.as_ref().unwrap().revenue_today;
    let (r_bar, r_cin) = (rev(&w, bar), rev(&w, cinema));
    let purse: f32 = people.iter().map(|c| w.citizens[*c].savings).sum();
    let grocery = w.buildings[bar].business.as_ref().unwrap().stock_of(Resource::Groceries);
    w.venue_tabs();
    assert!(rev(&w, bar) > r_bar, "a round at the bar");
    assert!(rev(&w, cinema) > r_cin, "a ticket at the cinema");
    assert!(w.buildings[bar].business.as_ref().unwrap().stock_of(Resource::Groceries) < grocery, "the drinks come out of its stock");
    assert!(w.economy.market.leisure.bookings_today > 0.0, "the film's distributor took its share");
    assert!(people.iter().map(|c| w.citizens[*c].savings).sum::<f32>() < purse);
    // A hungry diner is fed from the kitchen on arrival.
    let c = people[2];
    w.citizens[c].activity = Activity::Traveling { to: Some(diner), purpose: Purpose::Visit(LandUse::Entertainment), path: vec![w.building_entrance(diner).unwrap()], idx: 1 };
    let meals = w.buildings[diner].business.as_ref().unwrap().stock_of(Resource::BakedGoods);
    w.arrive(c);
    assert!(w.citizens[c].satiety > 0.6, "fed: {}", w.citizens[c].satiety);
    assert!(w.buildings[diner].business.as_ref().unwrap().stock_of(Resource::BakedGoods) < meals);
    assert_eq!(w.economy.market.leisure.meals_in_today, 1);
    // The kitchen restocks from the farms and mills (or out of town).
    w.buildings[diner].business.as_mut().unwrap().stock.clear();
    w.clock = crate::clock::SimClock::starting_at(4, 9, 0);
    w.hourly_economy();
    let z = w.buildings[diner].business.as_ref().unwrap();
    assert!(z.stock_of(Resource::Meat) + z.inbound_of(Resource::Meat) > 0.0, "meat ordered");
    assert!(z.stock_of(Resource::Flour) + z.inbound_of(Resource::Flour) > 0.0, "flour ordered");
    // Every dollar went through the door.
    assert_eq!(w.ledger_sum(), sum0);
}

#[test]
fn the_places_save_and_an_old_save_loads() {
    use BusinessKind as K;
    let all: Vec<K> = EATERIES.into_iter().chain(NIGHTS_OUT).collect();
    let (mut w, places) = open_places(&all);
    w.economy.market.leisure.note(K::Cinema, 20);
    let back = World::from_ron(&w.to_ron().unwrap()).expect("loads");
    for (b, k) in places.iter().zip(&all) {
        assert_eq!(kind_of(&back, *b), Some(*k));
        assert_eq!(back.buildings[*b].name, w.buildings[*b].name);
    }
    assert_eq!(back.economy.market.leisure, w.economy.market.leisure);
    // An old save from before the new kinds: loads, keeps its kinds, and
    // the new kinds' rows start empty.
    let mut old = World::from_ron(&legacy_save_text()).expect("the legacy town loads");
    assert!(old.buildings.values().all(|b| b.business.as_ref().is_none_or(|z| !z.kind.is_eat_or_go_out())));
    assert_eq!(old.economy.market.leisure, Default::default());
    for k in &all {
        assert_eq!(old.economy.cover_of(*k), None);
    }
    let mut minds = IdleMind;
    for _ in 0..200 {
        old.tick(&mut minds);
    }
}
