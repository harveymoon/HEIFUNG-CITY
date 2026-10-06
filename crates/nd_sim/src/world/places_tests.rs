//! [places] Taste, memory of places and the choice of where to go.

use super::*;
use crate::mind::IdleMind;
use super::market::VENUE_TAB;
use super::places::tab_factor;
use crate::places::{self, Reputation, Verdict};

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

/// A seed whose traits pass `pred`.
fn seed_where(pred: impl Fn([f32; 5]) -> bool) -> u64 {
    (1..50_000u64).find(|&s| pred(places::traits(s))).expect("a seed")
}

struct Town {
    w: World,
    dive: BuildingId,
    lounge: BuildingId,
    /// The first resident, the template for everyone else.
    first: CitizenId,
}

/// A street with homes and two venues side by side: a cheap, run-down
/// dive on a crime hot spot with overflowing bins, and a polished lounge.
fn night_town() -> Town {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Entertainment);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
    w.add_zone(square(10.0, -200.0, 190.0), LandUse::Commercial);
    let a = lot_near(&w, Vec2::new(60.0, 20.0), LandUse::Entertainment);
    w.build_on(a);
    let b = lot_near(&w, Vec2::new(60.0, 20.0), LandUse::Entertainment);
    w.build_on(b);
    let home = lot_near(&w, Vec2::new(-60.0, 20.0), LandUse::Residential);
    w.build_on(home);
    let shop = lot_near(&w, Vec2::new(60.0, -20.0), LandUse::Commercial);
    w.build_on(shop);
    finish_all(&mut w);
    let (dive, lounge) = (w.lots[a].building.unwrap(), w.lots[b].building.unwrap());
    for (id, name, tier, condition) in [(dive, "Rust Bucket Bar", Tier::Low, 0.45), (lounge, "Velvet Lounge", Tier::High, 1.0)] {
        let bld = &mut w.buildings[id];
        bld.name = name.to_string();
        bld.tier = tier;
        bld.condition = condition;
        bld.jobs = bld.jobs.max(4);
        bld.business = Some(Business::new(BusinessKind::Venue, bld.jobs));
    }
    // The dive's corner: a hot spot and bins nobody empties.
    let at = w.building_lot(dive).unwrap().center;
    w.society.crime.hotspots.push(crate::crime::Hotspot { pos: at, heat: 3.0 });
    w.society.trash.bins.insert(dive, crate::trash::Bin { stored: 160.0, capacity: 100.0, ..Default::default() });
    let mut minds = IdleMind;
    for _ in 0..900 {
        if !w.citizens.is_empty() {
            break;
        }
        w.tick(&mut minds);
    }
    let first = w.citizens.keys().next().expect("someone moved in");
    Town { w, dive, lounge, first }
}

impl Town {
    /// A new citizen like the first, with this seed, age and income,
    /// standing half-way between the two venues.
    fn person(&mut self, seed: u64, age: u8, income: f32) -> CitizenId {
        let mut c = self.w.citizens[self.first].clone();
        c.seed = seed;
        c.age = age;
        c.income = income;
        c.savings = 1000.0;
        c.household = None;
        c.places = Default::default();
        c.memories.clear();
        c.activity = Activity::Idle;
        c.inside = None;
        c.pos = (self.w.building_lot(self.dive).unwrap().center + self.w.building_lot(self.lounge).unwrap().center) * 0.5;
        self.w.citizens.insert(c)
    }

    /// Set the clock to 20:00 on day `day`, plus `minute`.
    fn evening(&mut self, day: u64, minute: u64) {
        self.w.clock = SimClock::starting_at(day, 20, minute);
    }

    /// How often out of `n` evenings `c` picks the dive.
    fn dive_share(&mut self, c: CitizenId, n: u64, explore: bool) -> f32 {
        let mut dive = 0;
        let mut any = 0;
        for i in 0..n {
            self.evening(3 + i / 50, i % 50);
            match self.w.choose_place(c, LandUse::Entertainment, explore) {
                Some(b) if b == self.dive => {
                    dive += 1;
                    any += 1;
                }
                Some(_) => any += 1,
                None => {}
            }
        }
        dive as f32 / any.max(1) as f32
    }

    /// Walk in through the door of `b`, then leave again.
    fn visit(&mut self, c: CitizenId, b: BuildingId) {
        let entrance = self.w.building_entrance(b).unwrap();
        self.w.citizens[c].savings = 1000.0;
        self.w.citizens[c].activity = Activity::Traveling { to: Some(b), purpose: Purpose::Visit(LandUse::Entertainment), path: vec![entrance], idx: 1 };
        self.w.arrive(c);
        assert_eq!(self.w.citizens[c].inside, Some(b), "got in");
        self.w.leave_building(c);
        self.w.citizens[c].activity = Activity::Idle;
        self.w.clock.tick += 1440;
    }
}

fn young_hedonist() -> u64 {
    seed_where(|t| t[2] > 0.85 && t[3] < 0.3 && t[0] > 0.5 && t[4] < 0.4)
}

fn staid() -> u64 {
    seed_where(|t| t[2] < 0.3 && t[3] > 0.6 && t[4] < 0.4)
}

#[test]
fn the_rich_and_old_avoid_a_sketchy_dive_the_young_hedonist_favours_it() {
    let mut t = night_town();
    let dive = t.w.place_character(t.dive).unwrap();
    let lounge = t.w.place_character(t.lounge).unwrap();
    assert!(dive.sketchiness() > 0.5 && dive.price < 0.3, "{dive:?}");
    assert!(lounge.polish > 0.8 && lounge.price > 0.7, "{lounge:?}");
    let old = t.person(staid(), 72, 300.0);
    let young = t.person(young_hedonist(), 22, 60.0);
    let old_share = t.dive_share(old, 300, false);
    let young_share = t.dive_share(young, 300, false);
    assert!(old_share < 0.15, "rich and old pick the dive {:.0}% of evenings", old_share * 100.0);
    assert!(young_share > 0.6, "the young hedonist picks it {:.0}% of evenings", young_share * 100.0);
}

#[test]
fn a_great_first_night_brings_them_back_and_habit_makes_a_regular() {
    let mut t = night_town();
    let seed = seed_where(|t| t[4] < 0.2 && t[2] > 0.4 && t[2] < 0.7 && t[3] > 0.3 && t[3] < 0.6);
    let c = t.person(seed, 35, 100.0);
    let before = t.dive_share(c, 300, false);
    // A great first night at the dive they would not have picked.
    let v = t.w.citizens[c].places.record(t.dive, LandUse::Entertainment, 0.85, t.w.clock.tick);
    assert_eq!(v, Verdict::Found);
    let after = t.dive_share(c, 300, false);
    assert!(after > before + 0.2 && after > 0.6, "back to the dive: {:.0}% before, {:.0}% after", before * 100.0, after * 100.0);

    // The whole loop: a young hedonist keeps going to the dive they like.
    let y = t.person(young_hedonist(), 23, 60.0);
    let mut visits = 0;
    while !t.w.citizens[y].places.get(t.dive).is_some_and(|m| m.regular || m.never) && visits < 10 {
        t.evening(10 + visits, 0);
        t.visit(y, t.dive);
        visits += 1;
    }
    let m = t.w.citizens[y].places.get(t.dive).cloned().unwrap();
    assert!(m.regular, "a regular after {visits} nights: {m:?} {:?}", t.w.citizens[y].memories.iter().map(|m| &m.text).collect::<Vec<_>>());
    assert!(t.w.citizens[y].memories.iter().any(|m| m.text.contains("Rust Bucket Bar is my place now")));
    assert!(t.w.reputation(t.dive).unwrap().rated >= visits as u32);
    t.w.daily_places();
    assert_eq!(t.w.reputation(t.dive).unwrap().regulars, 1);
    // And a regular mostly goes back.
    let back = t.dive_share(y, 200, false);
    assert!(back > 0.75, "a regular goes back {:.0}% of evenings", back * 100.0);
}

#[test]
fn a_terrible_night_means_never_again() {
    let mut t = night_town();
    let old = t.person(staid(), 74, 300.0);
    let mut nights = 0;
    while !t.w.citizens[old].places.is_never(t.dive) && nights < 6 {
        t.evening(10 + nights, 0);
        t.visit(old, t.dive);
        nights += 1;
    }
    assert!(t.w.citizens[old].places.is_never(t.dive), "sworn off after {nights} nights");
    assert!(t.w.citizens[old].memories.iter().any(|m| m.text.starts_with("Never going back to Rust Bucket Bar")), "{:?}", t.w.citizens[old].memories.iter().map(|m| &m.text).collect::<Vec<_>>());
    assert_eq!(t.dive_share(old, 200, false), 0.0, "never picked again");
    assert_eq!(t.dive_share(old, 50, true), 0.0, "not even looking for somewhere new");
    // With the lounge shut there is nowhere to go, rather than back there.
    t.w.buildings[t.lounge].business.as_mut().unwrap().closed = true;
    t.evening(20, 0);
    assert_eq!(t.w.choose_place(old, LandUse::Entertainment, false), None);
    t.w.daily_places();
    assert_eq!(t.w.reputation(t.dive).unwrap().never_again, 1);
}

#[test]
fn the_curious_find_new_places_and_explorers_only_look_at_new_ones() {
    let mut t = night_town();
    let curious = seed_where(|t| t[4] > 0.9 && t[2] > 0.3);
    let dull = seed_where(|t| t[4] < 0.1 && t[2] > 0.3);
    let mut share = |seed| {
        let c = t.person(seed, 40, 100.0);
        // Both know only the lounge, and like it.
        for k in 0..3 {
            t.w.citizens[c].places.record(t.lounge, LandUse::Entertainment, 0.6, k);
        }
        assert!(t.w.citizens[c].places.get(t.lounge).unwrap().regular);
        (c, t.dive_share(c, 400, false))
    };
    let (c, curious_share) = share(curious);
    let (_, dull_share) = share(dull);
    assert!(curious_share > 0.05 && curious_share > dull_share, "the curious try the new place {:.0}% vs {:.0}%", curious_share * 100.0, dull_share * 100.0);
    assert_eq!(t.dive_share(c, 50, true), 1.0, "looking for somewhere new finds the unknown place");
}

#[test]
fn regulars_meet_and_regulars_tip() {
    let mut t = night_town();
    let a = t.person(young_hedonist(), 24, 60.0);
    let b = t.person(young_hedonist(), 26, 60.0);
    for c in [a, b] {
        for k in 0..3 {
            t.w.citizens[c].places.record(t.dive, LandUse::Entertainment, 0.6, k);
        }
    }
    t.evening(5, 0);
    // a is in; b walks in and they get to know each other.
    let entrance = t.w.building_entrance(t.dive).unwrap();
    t.w.citizens[a].activity = Activity::Traveling { to: Some(t.dive), purpose: Purpose::Visit(LandUse::Entertainment), path: vec![entrance], idx: 1 };
    t.w.arrive(a);
    t.w.citizens[b].activity = Activity::Traveling { to: Some(t.dive), purpose: Purpose::Visit(LandUse::Entertainment), path: vec![entrance], idx: 1 };
    t.w.arrive(b);
    assert!(t.w.citizens[b].places.knows(a) && t.w.citizens[a].places.knows(b), "regulars at the same bar know each other");
    // The regular pays the round and a bit over.
    let (sa, money0) = (t.w.citizens[a].savings, t.w.buildings[t.dive].business.as_ref().unwrap().money);
    t.w.venue_tabs();
    let paid = sa - t.w.citizens[a].savings;
    assert!((paid - VENUE_TAB * tab_factor(Tier::Low) * (1.0 + places::REGULAR_TIP)).abs() < 1e-3, "paid {paid}");
    assert!(t.w.buildings[t.dive].business.as_ref().unwrap().money > money0);
    assert!(t.w.reputation(t.dive).unwrap().regular_revenue_today > 0.0);
}

#[test]
fn word_of_mouth_pulls_and_a_bad_name_repels() {
    let mut good = Reputation::default();
    let mut bad = Reputation::default();
    for _ in 0..30 {
        good.add(0.6);
        bad.add(-0.4);
    }
    bad.never_again = 12;
    good.regulars = 6;
    assert!(good.pull() > 0.15 && bad.pull() < -0.3, "{} {}", good.pull(), bad.pull());
}

/// Out in the evening (sometimes looking for somewhere new), home after.
struct OutMind(crate::SimRng);

impl MindProvider for OutMind {
    fn decide(&mut self, ctx: &MindContext) -> Option<Decision> {
        let h = ctx.clock.hour();
        let intent = if (18..23).contains(&h) && ctx.stage != crate::citizens::LifeStage::Child {
            match self.0.random_range(0..10) {
                0..=2 => Intent::Explore(LandUse::Entertainment),
                3..=7 => Intent::Visit(LandUse::Entertainment),
                _ => Intent::Visit(LandUse::Park),
            }
        } else {
            Intent::GoHome
        };
        Some(Decision { intent, thought: String::new(), memory: None })
    }

    fn name(&self) -> &'static str {
        "out"
    }
}

#[test]
fn places_survive_a_save_and_a_town_of_minds_replays_exactly() {
    fn run() -> World {
        let mut t = night_town();
        let mut minds = OutMind(crate::rng_for(5, 5));
        // Two evenings and nights of the town making its own choices.
        for _ in 0..(2 * 1440) {
            t.w.tick(&mut minds);
        }
        t.w
    }
    let (a, b) = (run(), run());
    let (ta, tb) = (a.to_ron().unwrap(), b.to_ron().unwrap());
    assert!(ta == tb, "two runs of the same town diverged");
    let rated: u32 = a.places.reputation.values().map(|r| r.rated).sum();
    assert!(rated > 0, "people went out and rated it");
    let back = World::from_ron(&ta).unwrap();
    for (id, c) in a.citizens.iter() {
        assert_eq!(back.citizens[id].places, c.places);
    }
    assert_eq!(back.places.reputation, a.places.reputation);
}

#[test]
fn friends_and_family_sometimes_come_along() {
    let mut t = night_town();
    let social = seed_where(|t| t[0] > 0.95);
    let a = t.person(social, 30, 100.0);
    let b = t.person(social + 1, 31, 100.0);
    let hid = t.w.households.insert(crate::citizens::Household { members: vec![a, b], home: t.w.citizens[a].home, money: 500.0, groceries: 10.0, ..Default::default() });
    for c in [a, b] {
        t.w.citizens[c].household = Some(hid);
        t.w.citizens[c].work = None;
        t.w.citizens[c].wakefulness = 1.0;
        t.w.citizens[c].asleep = false;
    }
    let mut together = false;
    for i in 0..40 {
        t.evening(4, i);
        t.w.citizens[b].activity = Activity::Idle;
        t.w.bring_company(a, t.dive, Purpose::Visit(LandUse::Entertainment));
        if t.w.citizens[b].activity.is_traveling() || t.w.citizens[b].to_car.is_some() {
            together = true;
            break;
        }
    }
    assert!(together, "a very sociable person brings their household along now and then");
    assert!(t.w.places.outings_together_total >= 1);
}
