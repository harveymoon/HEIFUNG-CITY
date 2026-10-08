//! [social] Friends and feuds, outings, groups and what they make.

use super::*;
use crate::mind::IdleMind;
use crate::places::{self, PlaceMemory};
use crate::social::*;

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

struct Town {
    w: World,
    bar: BuildingId,
    lounge: BuildingId,
    homes: Vec<BuildingId>,
    first: CitizenId,
}

/// A street with two venues, a row of homes and empty commercial land.
fn town(seed: u64) -> Town {
    let mut w = World::new(WorldConfig { seed, emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), dead_ends_lead_elsewhere: true, ..WorldConfig::default() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Entertainment);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
    w.add_zone(square(10.0, -200.0, 190.0), LandUse::Commercial);
    w.add_zone(square(-200.0, -200.0, 190.0), LandUse::Residential);
    let a = lot_near(&w, Vec2::new(60.0, 20.0), LandUse::Entertainment);
    w.build_on(a);
    let b = lot_near(&w, Vec2::new(60.0, 20.0), LandUse::Entertainment);
    w.build_on(b);
    let mut home_lots = Vec::new();
    for x in [-40.0, -120.0, -180.0] {
        let l = lot_near(&w, Vec2::new(x, 20.0), LandUse::Residential);
        w.build_on(l);
        home_lots.push(l);
    }
    finish_all(&mut w);
    let (bar, lounge) = (w.lots[a].building.unwrap(), w.lots[b].building.unwrap());
    for (id, name) in [(bar, "Blue Lantern Bar"), (lounge, "Velvet Lounge")] {
        let bld = &mut w.buildings[id];
        bld.name = name.to_string();
        bld.tier = Tier::Mid;
        bld.condition = 1.0;
        bld.jobs = bld.jobs.max(4);
        bld.business = Some(Business::new(BusinessKind::Venue, bld.jobs));
    }
    let homes: Vec<BuildingId> = home_lots.iter().filter_map(|l| w.lots[*l].building).collect();
    let mut minds = IdleMind;
    for _ in 0..900 {
        if !w.citizens.is_empty() {
            break;
        }
        w.tick(&mut minds);
    }
    let first = w.citizens.keys().next().expect("someone moved in");
    Town { w, bar, lounge, homes, first }
}

impl Town {
    /// A new citizen like the first, with this seed and age, in a
    /// household of their own at home `h`.
    fn person(&mut self, seed: u64, age: u8, h: usize) -> CitizenId {
        let home = self.homes[h % self.homes.len()];
        let mut c = self.w.citizens[self.first].clone();
        c.seed = seed;
        c.age = age;
        c.income = 90.0;
        c.savings = 300.0;
        c.places = Default::default();
        c.memories.clear();
        c.activity = Activity::AtHome;
        c.inside = Some(home);
        c.home = Some(home);
        c.work = None;
        c.mood = 0.6;
        c.drunk = 0.0;
        c.asleep = false;
        c.wakefulness = 1.0;
        c.to_car = None;
        let id = self.w.citizens.insert(c);
        let hid = self.w.households.insert(Household { members: vec![id], home: Some(home), money: 400.0, groceries: 5.0, ..Default::default() });
        self.w.citizens[id].household = Some(hid);
        self.w.buildings[home].households.push(hid);
        self.w.buildings[home].residents.push(id);
        id
    }

    fn at(&mut self, who: &[CitizenId], b: BuildingId) {
        for &c in who {
            let x = &mut self.w.citizens[c];
            x.activity = Activity::Visiting(b);
            x.inside = Some(b);
            x.next_decision_tick = u64::MAX / 2;
        }
    }

    /// `n` evenings at `b` together: 20:00 to 23:00 each.
    fn evenings(&mut self, who: &[CitizenId], b: BuildingId, n: u64) {
        let mut minds = IdleMind;
        for d in 0..n {
            self.w.clock = SimClock::starting_at(2 + d, 20, 0);
            self.at(who, b);
            for _ in 0..180 {
                self.w.tick(&mut minds);
                self.at(who, b);
            }
        }
    }

    fn make_friends(&mut self, a: CitizenId, b: CitizenId, affinity: f32) {
        let tick = self.w.clock.tick;
        for (x, y) in [(a, b), (b, a)] {
            let t = self.w.social.circles.entry(x).or_default().tie_or_new(y, tick, Met::Venue, Some(self.bar));
            t.affinity = affinity;
            t.familiarity = 0.6;
            t.trust = 0.6;
            t.last = tick;
        }
    }

    fn likes(&mut self, c: CitizenId, b: BuildingId, score: f32) {
        let lu = self.w.buildings[b].land_use;
        self.w.citizens[c].places.places.push(PlaceMemory { place: b, land_use: lu, visits: 5, last_visit: 0, score, best: score, worst: score, regular: true, never: false });
    }
}

/// A pair of seeds (thirty-year-olds of the middle class) whose
/// compatibility passes `pred`.
fn pair_where(pred: impl Fn(f32) -> bool, sociable: bool) -> (u64, u64) {
    for a in 1..400u64 {
        if sociable && places::traits(a)[0] < 0.6 {
            continue;
        }
        for b in 400..1200u64 {
            if sociable && places::traits(b)[0] < 0.6 {
                continue;
            }
            if pred(compatibility((a, 30, Tier::Mid), (b, 31, Tier::Mid))) {
                return (a, b);
            }
        }
    }
    panic!("no such pair");
}

#[test]
fn compatible_people_at_the_same_bar_become_friends() {
    let mut t = town(11);
    let (sa, sb) = pair_where(|c| c > 0.45, true);
    let a = t.person(sa, 30, 0);
    let b = t.person(sb, 31, 1);
    for c in [a, b] {
        t.w.citizens[c].income = 120.0;
    }
    let bar = t.bar;
    t.evenings(&[a, b], bar, 6);
    let tie = t.w.tie_between(a, b).expect("they got talking");
    assert!(tie.talks >= 3, "{tie:?}");
    assert!(tie.bond().is_friendly(), "compatible regulars become friends: {tie:?}");
    assert_eq!(t.w.social.bond(b, a), t.w.social.bond(a, b), "the tie is mutual");
    assert!(t.w.citizens[a].memories.iter().any(|m| m.text.contains("friends now")), "{:?}", t.w.citizens[a].memories);
    let notes = &t.w.circle_of(a).unwrap().talks;
    assert!(notes.iter().any(|n| n.with == b && n.place == Some(bar)), "they remember talking at the bar");
    assert!(t.w.social.recent.iter().any(|x| x.place == Some(bar)), "a bubble to show");
}

#[test]
fn incompatible_people_become_rivals() {
    let mut t = town(12);
    let (sa, sb) = pair_where(|c| c < -0.55, false);
    let a = t.person(sa, 30, 0);
    let b = t.person(sb, 31, 1);
    for c in [a, b] {
        t.w.citizens[c].mood = 0.35;
    }
    let bar = t.bar;
    t.evenings(&[a, b], bar, 10);
    let tie = t.w.tie_between(a, b).expect("they got talking");
    assert!(tie.bond().is_hostile(), "a bad match turns sour: {tie:?}");
    assert!(t.w.social.feuds_total >= 1);
}

#[test]
fn ties_fade_without_contact() {
    let mut t = town(13);
    let a = t.person(101, 30, 0);
    let b = t.person(202, 30, 1);
    t.make_friends(a, b, 0.5);
    assert!(t.w.social.bond(a, b).is_friendly());
    for d in 0..40u64 {
        t.w.clock = SimClock::starting_at(3 + d, 0, 0);
        t.w.fade_ties();
    }
    let still = t.w.tie_between(a, b).map(|x| x.bond());
    assert!(!still.is_some_and(|x| x.is_friendly()), "forty days apart and they are not friends any more: {still:?}");
    let aff = t.w.tie_between(a, b).map_or(0.0, |x| x.affinity);
    assert!(aff < 0.25, "{aff}");
}

#[test]
fn friends_meet_up_at_a_place_they_both_like() {
    let mut t = town(14);
    let social = |s: u64| places::traits(s)[0] > 0.85;
    let sa = (1..5000u64).find(|s| social(*s)).unwrap();
    let sb = (sa + 1..5000u64).find(|s| social(*s)).unwrap();
    let a = t.person(sa, 30, 0);
    let b = t.person(sb, 32, 2);
    t.make_friends(a, b, 0.7);
    let bar = t.bar;
    t.likes(a, bar, 0.6);
    t.likes(b, bar, 0.5);
    let mut minds = IdleMind;
    let mut met = false;
    'days: for d in 0..30u64 {
        t.w.clock = SimClock::starting_at(2 + d, 18, 25);
        for c in [a, b] {
            let home = t.w.citizens[c].home;
            let x = &mut t.w.citizens[c];
            x.activity = Activity::AtHome;
            x.inside = home;
            x.next_decision_tick = u64::MAX / 2;
        }
        t.w.plan_outings();
        if t.w.social.today.meetups == 0 {
            continue;
        }
        for _ in 0..300 {
            t.w.tick(&mut minds);
            let here = |c: CitizenId| matches!(t.w.citizens[c].activity, Activity::Visiting(x) if x == bar);
            if here(a) && here(b) {
                met = true;
                break 'days;
            }
        }
    }
    assert!(met, "the friends got together at their bar: {:?} / {:?}", t.w.citizens[a].activity, t.w.citizens[b].activity);
    assert!(t.w.citizens[a].memories.iter().any(|m| m.text.contains("Met up with")));
}

#[test]
fn enemies_avoid_each_others_places() {
    let mut t = town(15);
    let a = t.person(303, 30, 0);
    let b = t.person(404, 30, 1);
    let (bar, lounge) = (t.bar, t.lounge);
    t.likes(a, bar, 0.4);
    t.likes(a, lounge, 0.4);
    t.at(&[b], bar);
    let share = |t: &mut Town| {
        let mut n = 0;
        for i in 0..200u64 {
            t.w.clock = SimClock::starting_at(3 + i / 50, 20, i % 50);
            if t.w.choose_place(a, LandUse::Entertainment, false) == Some(bar) {
                n += 1;
            }
        }
        n as f32 / 200.0
    };
    let before = share(&mut t);
    for (x, y) in [(a, b), (b, a)] {
        let tie = t.w.social.circles.entry(x).or_default().tie_or_new(y, 0, Met::Venue, Some(bar));
        tie.affinity = -0.8;
        tie.familiarity = 0.5;
    }
    assert_eq!(t.w.social.bond(a, b), Bond::Enemy);
    assert!(t.w.social_pull(a, bar, &[b]) < -0.4);
    let after = share(&mut t);
    assert!(after < before * 0.6, "with an enemy at the bar, the lounge it is: {before} -> {after}");
}

fn band_seed(skip: u64) -> u64 {
    (skip..50_000u64).find(|&s| interests(s, 24, Tier::Mid)[GroupKind::Band.index()] >= 0.6 && places::traits(s)[0] > 0.4).unwrap()
}

#[test]
fn friends_who_love_music_form_a_band_that_plays_a_gig() {
    let mut t = town(16);
    let s1 = band_seed(1);
    let s2 = band_seed(s1 + 1);
    let s3 = band_seed(s2 + 1);
    let a = t.person(s1, 24, 0);
    let b = t.person(s2, 25, 1);
    let c = t.person(s3, 23, 2);
    t.make_friends(a, b, 0.6);
    t.make_friends(a, c, 0.6);
    t.make_friends(b, c, 0.5);
    let bar = t.bar;
    for x in [a, b, c] {
        t.likes(x, bar, 0.5);
    }
    for d in 0..40u64 {
        t.w.clock = SimClock::starting_at(3 + d, 0, 0);
        t.w.form_groups();
        if !t.w.social.groups.is_empty() {
            break;
        }
    }
    let g = t.w.social.groups.first().expect("a group formed").clone();
    assert_eq!(g.kind, GroupKind::Band, "{g:?}");
    assert!(g.members.len() >= 3 && g.members.contains(&a));
    assert_eq!(g.base, Some(bar), "they play where they drink");
    assert!(t.w.circle_of(a).unwrap().groups.contains(&g.id));
    // Momentum to spare: a gig.
    let rating = t.w.places.reputation.get(&bar).map_or(0.0, |r| r.rating);
    t.w.social.group_mut(g.id).unwrap().momentum = 5.0;
    t.w.produce();
    let gig = t.w.social.gigs.first().expect("a gig booked").clone();
    assert_eq!(gig.venue, bar);
    assert!(t.w.places.reputation.get(&bar).unwrap().rating > rating, "a gig is good for the bar's name");
    assert!(t.w.social.news.iter().any(|n| n.text.contains("tonight")));
    // Tonight the gig pulls people in, friends of the band most.
    t.w.clock.tick = gig.from + 10;
    let stranger = t.person(77, 40, 0);
    let fan = t.person(78, 26, 1);
    t.make_friends(fan, a, 0.6);
    assert!(t.w.social_pull(fan, bar, &[]) > t.w.social_pull(stranger, bar, &[]));
    assert!(t.w.social_pull(stranger, bar, &[]) > 0.2);
}

fn kind_seed(kind: GroupKind, skip: u64) -> u64 {
    (skip..200_000u64)
        .find(|&s| {
            let i = interests(s, 40, Tier::Mid);
            let top = GroupKind::ALL.iter().max_by(|x, y| i[x.index()].total_cmp(&i[y.index()])).unwrap();
            *top == kind && i[kind.index()] >= 0.5
        })
        .unwrap()
}

/// Three friends whose strongest passion is `kind`, formed into a group.
fn group_of(t: &mut Town, kind: GroupKind) -> u32 {
    let s1 = kind_seed(kind, 1);
    let s2 = kind_seed(kind, s1 + 1);
    let s3 = kind_seed(kind, s2 + 1);
    let a = t.person(s1, 40, 0);
    let b = t.person(s2, 41, 0);
    let c = t.person(s3, 42, 1);
    t.make_friends(a, b, 0.6);
    t.make_friends(a, c, 0.6);
    t.make_friends(b, c, 0.6);
    for d in 0..60u64 {
        t.w.clock = SimClock::starting_at(3 + d, 0, 0);
        t.w.form_groups();
        if let Some(g) = t.w.social.groups.iter().find(|g| g.kind == kind) {
            return g.id;
        }
    }
    panic!("no {kind:?} formed: {:?}", t.w.social.groups);
}

#[test]
fn a_crew_paints_a_mural_and_gardeners_take_an_empty_lot() {
    let mut t = town(17);
    let crew = group_of(&mut t, GroupKind::StreetArt);
    t.w.social.group_mut(crew).unwrap().momentum = 5.0;
    t.w.produce();
    let m = t.w.social.murals.first().expect("a mural").clone();
    assert_eq!(m.group, crew);
    assert!(m.building.is_some_and(|b| t.w.buildings.contains_key(b)));
    assert!(t.w.social.group(crew).unwrap().works.iter().any(|w| matches!(w, Work::Mural { .. })));

    let growers = group_of(&mut t, GroupKind::Garden);
    t.w.social.group_mut(growers).unwrap().momentum = 5.0;
    t.w.produce();
    let g = t.w.social.gardens.first().expect("a garden").clone();
    assert!(t.w.lots[g.lot].building.is_none());
    // Built over: the garden is gone.
    t.w.build_on(g.lot);
    t.w.clock = SimClock::starting_at(80, 0, 0);
    t.w.daily_groups();
    assert!(t.w.social.gardens.is_empty());
}

#[test]
fn a_startup_opens_a_real_business() {
    let mut t = town(18);
    let g = group_of(&mut t, GroupKind::Startup);
    let members = t.w.social.group(g).unwrap().members.clone();
    for m in &members {
        t.w.citizens[*m].savings = 800.0;
    }
    let before = t.w.buildings.len();
    t.w.social.group_mut(g).unwrap().momentum = 10.0;
    t.w.produce();
    assert_eq!(t.w.buildings.len(), before + 1, "a new business site: {:?}", t.w.social.news);
    let Some(Work::Business { building, .. }) = t.w.social.group(g).unwrap().works.first().cloned() else { panic!("no business") };
    let b = &t.w.buildings[building];
    assert!(b.business.is_some() && b.construction.is_some());
    assert_eq!(b.business.as_ref().unwrap().owner, Some(t.w.social.group(g).unwrap().founder));
    assert!(members.iter().all(|m| t.w.citizens[*m].savings < 800.0), "they put their money in");
}

#[test]
fn a_neighbourhood_petitions_the_player_and_protests_when_ignored() {
    let mut t = town(19);
    let g = group_of(&mut t, GroupKind::Neighbourhood);
    t.w.social.group_mut(g).unwrap().momentum = 5.0;
    let day = t.w.clock.day();
    t.w.produce();
    let p = t.w.social.petitions.first().expect("a petition").clone();
    assert_eq!(p.group, g);
    assert_eq!(p.state, PetitionState::Open);
    assert!(p.text.contains("asks the city"), "{}", p.text);
    // The ask in this town: they are all out of work.
    assert_eq!(p.ask, Ask::Jobs, "{}", p.text);
    t.w.social.group_mut(g).unwrap().momentum = 1.0;
    for d in 1..=4 {
        t.w.clock = SimClock::starting_at(day + d, 0, 0);
        t.w.check_petitions();
    }
    let p = t.w.social.petitions.first().unwrap();
    assert_eq!(p.state, PetitionState::Protested, "{p:?}");
    assert!(t.w.social.group(g).unwrap().works.iter().any(|w| matches!(w, Work::Protest { .. })));
    assert!(t.w.social.news.iter().any(|n| n.text.contains("protest")));
    // Jobs come: the petition is answered.
    let bar = t.bar;
    let members = t.w.social.group(g).unwrap().members.clone();
    t.w.buildings[bar].jobs = 40;
    for m in members {
        t.w.assign_job(m, bar);
    }
    t.w.clock = SimClock::starting_at(day + 5, 0, 0);
    t.w.check_petitions();
    assert_eq!(t.w.social.petitions.first().unwrap().state, PetitionState::Answered);
}

#[test]
fn partners_move_in_together_and_friends_find_each_other_jobs() {
    let mut t = town(20);
    let a = t.person(505, 30, 0);
    let b = t.person(606, 31, 1);
    t.make_friends(a, b, 0.8);
    for (x, y) in [(a, b), (b, a)] {
        t.w.social.circles.get_mut(&x).unwrap().tie_mut(y).unwrap().partner = true;
    }
    t.w.clock = SimClock::starting_at(6, 0, 0);
    t.w.move_in_together();
    assert_eq!(t.w.citizens[a].household, t.w.citizens[b].household, "living together");
    assert_eq!(t.w.citizens[a].home, t.w.citizens[b].home);
    let hh = t.w.citizens[a].household.unwrap();
    assert_eq!(t.w.households[hh].size(), 2);
    // A job through a friend.
    let c = t.person(707, 30, 2);
    let d = t.person(808, 30, 2);
    let bar = t.bar;
    t.w.assign_job(d, bar);
    t.w.buildings[bar].jobs = t.w.buildings[bar].workers.len() as u16 + 3;
    t.make_friends(c, d, 0.6);
    t.w.friends_help();
    assert_eq!(t.w.citizens[c].work, Some(bar), "{:?}", t.w.citizens[c].memories);
}

#[test]
fn the_same_seed_makes_the_same_friends() {
    let run = || {
        let mut t = town(21);
        let ids: Vec<CitizenId> = (0..6).map(|k| t.person(900 + k * 37, 25 + k as u8 * 3, k as usize)).collect();
        let bar = t.bar;
        t.evenings(&ids, bar, 4);
        t.w.social.circles.clone()
    };
    let (x, y) = (run(), run());
    assert!(x.values().map(|c| c.ties.len()).sum::<usize>() > 4, "people talked");
    assert_eq!(x, y);
}

#[test]
fn an_old_save_without_the_social_layer_loads() {
    let mut t = town(22);
    let a = t.person(101, 30, 0);
    let b = t.person(202, 30, 1);
    t.make_friends(a, b, 0.5);
    let text = t.w.to_ron().unwrap();
    // Cut the social block and the config flag, as a save from before has neither.
    let start = text.find("\n    social: (").expect("social is saved");
    let mut depth = 0i32;
    let mut end = start;
    for (i, ch) in text[start..].char_indices() {
        match ch {
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    let end = if text.as_bytes().get(end) == Some(&b',') { end + 1 } else { end };
    let old = format!("{}{}", &text[..start], &text[end..]);
    let old = old.replace("        social: true,\n", "");
    assert!(!old.contains("circles"), "the block is gone");
    let w = World::from_ron(&old).expect("loads");
    assert!(w.social.circles.is_empty());
    assert!(w.config.social, "the layer is on for an old save");
    // And the new save keeps the ties.
    let back = World::from_ron(&text).unwrap();
    assert!(back.social.bond(a, b).is_friendly());
}
