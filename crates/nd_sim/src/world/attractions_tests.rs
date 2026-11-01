//! [attractions] The landmark attractions: one per city, a site of several
//! plots, the pier only on the sea shore and out over the water, the
//! observatory only on high ground, the lift to the land value round them,
//! and visitors (the town's own and tourists) coming through the door.

use super::*;
use crate::attractions::{OBSERVATORY_RISE, PIER_MIN_LENGTH};
use crate::landscape::{LandscapeParams, LandscapePreset, WaterKind};
use crate::mind::IdleMind;
use crate::services::{BuildCategory, Placeable, ServiceKind};
use crate::tourism::{AttractionKind, Goal};

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

fn landmark(k: ServiceKind) -> Placeable {
    Placeable::Service(k, Tier::Mid)
}

/// An avenue between two back streets with housing zoned in the deep
/// blocks either side, flat ground, plenty of money.
fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 5_000_000.0;
    for y in [-150.0f32, 0.0, 150.0] {
        w.add_road(&[Vec2::new(-700.0, y), Vec2::new(700.0, y)], 1, 1);
    }
    w.add_zone(square(-690.0, -142.0, 1380.0, 134.0), LandUse::Residential);
    w.add_zone(square(-690.0, 8.0, 1380.0, 134.0), LandUse::Residential);
    w
}

/// A vacant residential lot by the avenue, nearest `p`.
fn home_lot(w: &World, p: Vec2) -> LotId {
    w.lots
        .iter()
        .filter(|(_, l)| l.land_use == LandUse::Residential && l.usable() && l.building.is_none() && l.car_access())
        .min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p)))
        .map(|(id, _)| id)
        .expect("a residential lot")
}

fn finish_all(w: &mut World) {
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
}

#[test]
fn a_landmark_takes_several_plots_and_a_city_has_one_of_each() {
    let mut w = town();
    let what = landmark(ServiceKind::ArtMuseum);
    let lot = home_lot(&w, Vec2::new(-300.0, -30.0));
    assert!(w.lots[lot].area() * 2.0 < what.min_area(), "a plot is a fraction of a museum ({:.0} m²)", w.lots[lot].area());
    // On its own the plot is too small...
    let alone = w.placement_problem(what, lot).expect("one plot alone is refused");
    assert!(alone.contains("too small"), "{alone}");
    // ...so the site takes in its neighbours.
    let site = w.placement_site(what, w.lots[lot].center).unwrap();
    assert!(site.joined.len() >= 2, "three or more plots joined: {}", site.note());
    assert!(w.site_problem(what, &site).is_none(), "{:?}", w.site_problem(what, &site));
    let lots0 = w.lots.len();
    let id = w.place_building(what, w.lots[lot].center).expect("an art museum over several plots");
    assert!(w.lots.len() + 2 <= lots0, "plots were joined into one");
    assert!(w.lots[w.buildings[id].lot].area() >= what.min_area() * 0.98);
    assert_eq!(w.landmark(ServiceKind::ArtMuseum), Some(id));
    // A second art museum is refused anywhere, and the reason says so.
    let far = home_lot(&w, Vec2::new(400.0, 30.0));
    let err = w.place_building(what, w.lots[far].center).expect_err("one per city");
    assert!(err.contains("one per city") && err.contains(&w.buildings[id].name), "{err}");
    let hover = w.placement_problem_at(what, w.lots[far].center).expect_err("the hover says so too");
    assert!(hover.contains("one per city"), "{hover}");
    // Another landmark goes down beside it.
    w.place_building(landmark(ServiceKind::HistoryMuseum), w.lots[far].center).expect("a different landmark");
    // Bulldozed, the city may build an art museum again.
    w.bulldoze_building(id).expect("the city's own");
    let again = home_lot(&w, Vec2::new(-500.0, -30.0));
    assert!(w.placement_problem_at(what, w.lots[again].center).is_ok(), "{:?}", w.placement_problem_at(what, w.lots[again].center).err());
}

#[test]
fn every_landmark_wants_more_than_one_ordinary_plot() {
    let w = town();
    let mut areas: Vec<f32> = w.lots.values().filter(|l| l.usable()).map(|l| l.area()).collect();
    areas.sort_by(|a, b| a.total_cmp(b));
    let typical = areas[areas.len() / 2];
    for k in ServiceKind::LANDMARKS {
        let p = landmark(k);
        assert!(p.min_area() > typical * 2.5, "{k:?} needs {:.0} m², a plot here is about {typical:.0}", p.min_area());
        assert!(!p.has_tiers(), "{k:?} comes in one size");
        assert_eq!(p.with_tier(Tier::High), p, "{k:?} stays at its one size");
        assert_eq!(Placeable::parse(k.key(), Some(Tier::High)), Some(p), "{k:?} by its key, at its one size");
        assert!(BuildCategory::Attractions.items().contains(&p), "{k:?} under Attractions");
    }
}

#[test]
fn the_grand_park_lays_out_walkways_and_benches_joined_to_the_streets() {
    let mut w = town();
    let what = landmark(ServiceKind::GrandPark);
    let lot = home_lot(&w, Vec2::new(0.0, -30.0));
    let id = w.place_building(what, w.lots[lot].center).expect("a grand park");
    let lot = w.buildings[id].lot;
    assert!(w.lots[lot].area() >= what.min_area() * 0.98);
    let plan = w.lots[lot].park.clone().expect("its walkways are laid out");
    assert!(plan.paths.len() >= 2 && plan.benches.len() >= 6, "{} paths, {} benches", plan.paths.len(), plan.benches.len());
    assert!(plan.features.iter().any(|f| matches!(f.kind, crate::park::ParkFeature::Lake | crate::park::ParkFeature::Pond)), "a lake");
    assert!(w.walk.benches_in(lot).count() >= 6, "the benches are on the walk graph");
    // The park's walkways are reached on foot from the street.
    let deep = plan.paths[0][plan.paths[0].len() / 2];
    let (route, _) = w.walk.route(Vec2::new(-400.0, -5.0), deep).expect("a way in on foot");
    assert!(route.last().unwrap().distance(deep) < 6.0);
    // The Path tool sees its gates as attraction grounds.
    assert!(w.grounds().iter().any(|g| g.kind == crate::walk::GroundsKind::Attraction && g.lot == Some(lot)));
}

// ── the pier ────────────────────────────────────────────────────────────────

fn coast_world(seed: u64) -> World {
    let mut w = World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::Coast, seed), ..WorldConfig::test_town() });
    w.economy.treasury = 5_000_000.0;
    w
}

/// A straight stretch of coast near the middle with open sea off it and
/// dry land behind: a point on the waterline, inland, and along the shore.
fn coast_spot(w: &World) -> (Vec2, Vec2, Vec2) {
    let sea = w.landscape.sea.as_ref().expect("a coast has a sea");
    let mut best: Option<(f32, Vec2, Vec2, Vec2)> = None;
    for seg in sea.coast.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        if a.distance(b) < 1.0 {
            continue;
        }
        let p = (a + b) * 0.5;
        let along = (b - a).normalize();
        let mut inland = Vec2::new(-along.y, along.x);
        if w.landscape.is_water(p + inland * 15.0) {
            inland = -inland;
        }
        let ok = (-4..=4).all(|k| {
            let q = p + along * (k as f32 * 45.0);
            let open_sea = (1..=6).all(|j| w.landscape.water_at(q - inland * (j as f32 * 25.0)).is_some_and(|h| h.kind == WaterKind::Sea));
            let dry_back = (1..=11).all(|j| !w.landscape.is_water(q + inland * (j as f32 * 20.0)));
            open_sea && dry_back
        });
        if !ok {
            continue;
        }
        let d = p.length();
        if best.is_none_or(|(bd, ..)| d < bd) {
            best = Some((d, p, inland, along));
        }
    }
    let (_, p, inland, along) = best.expect("a straight coast near the middle");
    (p, inland, along)
}

#[test]
fn a_pier_goes_only_on_the_sea_shore_and_runs_out_over_the_water() {
    let mut w = coast_world(7);
    let (p, inland, along) = coast_spot(&w);
    w.landscape.reshape_for_test(move |q, h| {
        let d = q - p;
        if d.length() > 600.0 || d.dot(inland) < 0.0 {
            return h;
        }
        3.5 + 0.01 * d.dot(inland)
    });
    let mid = p + inland * 75.0;
    w.add_road(&[mid - along * 160.0, mid + along * 160.0], 1, 1);
    let a = p - inland * 30.0 - along * 150.0;
    let b = p - inland * 30.0 + along * 150.0;
    w.add_zone(vec![a, b, b + inland * 200.0, a + inland * 200.0], LandUse::Commercial);
    let what = landmark(ServiceKind::Pier);
    // Inland, across the street from the sea: refused, and the hover says why.
    let inland_lot = w
        .lots
        .iter()
        .filter(|(_, l)| l.usable() && (l.center - p).dot(inland) > 85.0 && (l.center - p).dot(along).abs() < 60.0)
        .map(|(id, _)| id)
        .next()
        .expect("a lot inland");
    let err = w.placement_problem_at(what, w.lots[inland_lot].center).expect_err("no pier inland");
    assert!(err.contains("sea shore"), "{err}");
    // On the water: the deck runs out over open sea on its piles.
    let shore_lot = w
        .lots
        .iter()
        .filter(|(_, l)| l.usable() && l.shore.is_waterfront() && (l.center - p).dot(along).abs() < 60.0)
        .min_by(|x, y| (x.1.center - p).dot(along).abs().total_cmp(&(y.1.center - p).dot(along).abs()))
        .map(|(id, _)| id)
        .expect("a lot on the water");
    let id = w.place_building(what, w.lots[shore_lot].center).expect("a pier on the sea shore");
    let deck = w.attractions.piers.get(&id).cloned().expect("its deck");
    assert!(deck.length() >= PIER_MIN_LENGTH, "{:.0} m out", deck.length());
    assert!(deck.seaward.dot(-inland) > 0.7, "out to sea");
    for q in [deck.tip, deck.head_centre(), deck.root + deck.seaward * (deck.length() * 0.5)] {
        assert!(w.landscape.water_at(q).is_some_and(|h| h.kind == WaterKind::Sea), "over the sea at {q:?}");
        assert!(w.surface_height(q) > crate::landscape::WATER_LEVEL + 0.3, "walkers stand on the boards, not the water");
    }
    // Its boardwalk is on the walk graph out to the head, with benches.
    let lot = w.buildings[id].lot;
    assert!(w.walk.benches_in(lot).count() >= 4);
    let (route, _) = w.walk.route(mid + along * 100.0, deck.head_centre()).expect("walked out to the head");
    assert!(route.last().unwrap().distance(deck.head_centre()) < deck.head * 0.6, "the walk reaches the head");
    // A second pier: one per city.
    let other = w.lots.iter().filter(|(lid, l)| *lid != lot && l.usable() && l.shore.is_waterfront() && l.building.is_none()).map(|(id, _)| id).next();
    if let Some(o) = other {
        let err = w.placement_problem_at(what, w.lots[o].center).expect_err("one pier");
        assert!(err.contains("one per city"), "{err}");
    }
    // Bulldozed: the deck goes from the built ground too.
    let head = deck.head_centre();
    w.bulldoze_building(id).unwrap();
    assert!(w.attractions.piers.is_empty());
    assert!(w.surface_height(head) < crate::landscape::WATER_LEVEL + 0.3, "no boards left over the water");
}

#[test]
fn a_pier_refuses_a_flat_map() {
    let w = town();
    let lot = home_lot(&w, Vec2::new(0.0, 30.0));
    let site = w.placement_site(landmark(ServiceKind::Pier), w.lots[lot].center).unwrap();
    let why = w.site_problem(landmark(ServiceKind::Pier), &site).expect("no sea");
    assert!(why.contains("sea"), "{why}");
}

// ── the observatory ─────────────────────────────────────────────────────────

#[test]
fn an_observatory_goes_only_on_high_ground() {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: LandscapeParams::preset(LandscapePreset::Plains, 7), ..WorldConfig::test_town() });
    w.economy.treasury = 5_000_000.0;
    // A dry spot, the ground replaced by a plain with a plateau to the north.
    let mut at = None;
    'find: for y in (-600..=600).step_by(50) {
        for x in (-600..=600).step_by(50) {
            let q = Vec2::new(x as f32, y as f32);
            if w.landscape.water_distance(q) > 150.0 && q.length() < 700.0 {
                at = Some(q);
                break 'find;
            }
        }
    }
    let at = at.expect("dry ground");
    w.landscape.reshape_for_test(move |q, h| {
        let d = q - at;
        if d.x.abs() > 500.0 || d.y.abs() > 500.0 {
            return h;
        }
        10.0 + 30.0 * ((d.y - 60.0) / 100.0).clamp(0.0, 1.0)
    });
    // A long street down on the plain, a short one on the plateau.
    w.add_road(&[at + Vec2::new(-300.0, -120.0), at + Vec2::new(300.0, -120.0)], 1, 1);
    w.add_road(&[at + Vec2::new(-90.0, 260.0), at + Vec2::new(90.0, 260.0)], 1, 1);
    w.add_zone(vec![at + Vec2::new(-290.0, -190.0), at + Vec2::new(290.0, -190.0), at + Vec2::new(290.0, -50.0), at + Vec2::new(-290.0, -50.0)], LandUse::Residential);
    w.add_zone(vec![at + Vec2::new(-85.0, 200.0), at + Vec2::new(85.0, 200.0), at + Vec2::new(85.0, 320.0), at + Vec2::new(-85.0, 320.0)], LandUse::Residential);
    let mid = w.town_median_height().expect("a town on real ground");
    assert!(mid < 20.0, "most of the town is down on the plain ({mid:.1} m)");
    let what = landmark(ServiceKind::Observatory);
    let low = home_lot(&w, at + Vec2::new(0.0, -100.0));
    let err = w.placement_problem_at(what, w.lots[low].center).expect_err("not on the plain");
    assert!(err.contains("high ground") && err.contains("needs"), "{err}");
    let high = home_lot(&w, at + Vec2::new(0.0, 240.0));
    let rise = w.rise_above_town(w.lots[high].center).unwrap();
    assert!(rise >= OBSERVATORY_RISE, "the plateau stands {rise:.1} m above the town");
    let id = w.place_building(what, w.lots[high].center).expect("an observatory on the plateau");
    assert!(w.rise_above_town(w.lots[w.buildings[id].lot].center).unwrap() >= OBSERVATORY_RISE);
    // On a flat map there is no high ground at all.
    let flat = town();
    let lot = home_lot(&flat, Vec2::ZERO);
    let err = flat.placement_problem_at(what, flat.lots[lot].center).expect_err("flat");
    assert!(err.contains("flat"), "{err}");
}

// ── land value ──────────────────────────────────────────────────────────────

/// Mean land value at the lots whose centres lie between `r0` and `r1`
/// metres from `c`.
fn ring_value(w: &World, c: Vec2, r0: f32, r1: f32) -> f32 {
    let v: Vec<f32> = w.lots.values().filter(|l| (r0..r1).contains(&l.center.distance(c))).map(|l| w.land_value.at(l.center)).collect();
    assert!(!v.is_empty(), "lots between {r0} and {r1} m");
    v.iter().sum::<f32>() / v.len() as f32
}

/// The lift `what` put down on the lot nearest `site` gives the lots
/// between `r0` and `r1` metres from it, against the same town without it.
fn lift(what: Placeable, site: Vec2, r0: f32, r1: f32) -> f32 {
    let mut w = town();
    w.update_land_value();
    let lot = home_lot(&w, site);
    let id = w.place_building(what, w.lots[lot].center).expect("placed");
    finish_all(&mut w);
    assert!(w.buildings[id].operating());
    let c = w.lots[w.buildings[id].lot].center;
    let mut base = town();
    base.update_land_value();
    let before = ring_value(&base, c, r0, r1);
    w.update_land_value();
    ring_value(&w, c, r0, r1) - before
}

#[test]
fn a_landmark_visibly_lifts_the_land_round_it_further_than_a_monument() {
    let site = Vec2::new(0.0, -30.0);
    let park = landmark(ServiceKind::GrandPark);
    let near = lift(park, site, 0.0, 140.0);
    let far = lift(park, site, 200.0, 340.0);
    assert!(near > 0.08, "the blocks round a grand park rise clearly: +{near:.3}");
    assert!(far > 0.03, "and the quarter beyond: +{far:.3}");
    let museum = lift(landmark(ServiceKind::ScienceMuseum), site, 0.0, 140.0);
    assert!(museum > 0.06, "a museum lifts its blocks too: +{museum:.3}");
    // A monument's lift is gone a couple of hundred metres out.
    let monument = lift(landmark(ServiceKind::Monument), site, 240.0, 340.0);
    let landmark_far = lift(park, site, 240.0, 340.0);
    assert!(landmark_far > monument + 0.02, "the landmark reaches further: +{landmark_far:.3} against a monument's +{monument:.3}");
}

// ── visitors ────────────────────────────────────────────────────────────────

/// Homes along the avenue with people living in them, and a zoo on the
/// other side, open (its jobs waived: nobody here is looking for work).
fn visited_town() -> (World, BuildingId) {
    let mut w = town();
    for x in [-300.0f32, -260.0, -220.0] {
        let lot = home_lot(&w, Vec2::new(x, 30.0));
        w.build_on(lot);
    }
    finish_all(&mut w);
    let lot = home_lot(&w, Vec2::new(-120.0, -30.0));
    let zoo = w.place_building(landmark(ServiceKind::Zoo), w.lots[lot].center).expect("a zoo");
    finish_all(&mut w);
    let mut minds = IdleMind;
    for _ in 0..900 {
        if w.citizens.len() >= 3 {
            break;
        }
        w.tick(&mut minds);
    }
    assert!(!w.citizens.is_empty(), "people moved in");
    w.buildings[zoo].jobs = 0;
    (w, zoo)
}

#[test]
fn townsfolk_spend_an_afternoon_at_a_landmark_and_pay_at_the_door() {
    let (mut w, zoo) = visited_town();
    w.clock = crate::clock::SimClock::starting_at(3, 13, 0);
    let cid = w.citizens.keys().next().unwrap();
    w.citizens[cid].savings = 500.0;
    w.citizens[cid].activity = Activity::Idle;
    assert_eq!(w.choose_place(cid, LandUse::Park, false), Some(zoo), "a day out finds the zoo");
    w.travel_to(cid, Some(zoo), crate::citizens::Purpose::Visit(LandUse::Park));
    let mut minds = IdleMind;
    for _ in 0..240 {
        if w.attractions.book(zoo).visits_today > 0 {
            break;
        }
        w.buildings[zoo].jobs = 0;
        w.tick(&mut minds);
    }
    let book = w.attractions.book(zoo);
    assert!(book.visits_today >= 1, "the visit was counted");
    assert!(book.takings_today >= ServiceKind::Zoo.admission(), "a ticket was bought: {:.0}", book.takings_today);
    // After hours it is no day out.
    w.clock = crate::clock::SimClock::starting_at(3, 21, 0);
    w.buildings[zoo].jobs = 0;
    assert_ne!(w.choose_place(cid, LandUse::Park, false), Some(zoo), "the zoo shuts at six");
}

#[test]
fn tourists_come_to_see_a_landmark_and_are_counted() {
    let (mut w, zoo) = visited_town();
    w.clock = crate::clock::SimClock::starting_at(3, 10, 0);
    w.refresh_attractions();
    let sight = w.tourism.attractions.iter().find(|a| a.kind == AttractionKind::Zoo).cloned().expect("the zoo is a sight");
    assert!(sight.score >= ServiceKind::Zoo.tourist_pull() * 0.5, "it draws: {:.1}", sight.score);
    assert!(w.tourism.attractiveness >= sight.score);
    let hotel = w.buildings.iter().find(|(id, b)| *id != zoo && b.homes > 0).map(|(id, _)| id).unwrap();
    let start = w.building_entrance(hotel).unwrap();
    let tid = w.tourism.tourists.insert(crate::tourism::Tourist {
        seed: 11,
        age: 35,
        class: Tier::Mid,
        pos: start,
        prev_pos: start,
        heading: 0.0,
        prev_heading: 0.0,
        hotel,
        via: crate::tourism::Arrival::Road,
        port: None,
        arrived_tick: w.clock.tick,
        leave_day: w.clock.day() + 2,
        money: 300.0,
        spent: 0.0,
        state: crate::tourism::TouristState::InHotel,
        next_tick: u64::MAX,
        sights: 0,
        commuter: None,
    });
    w.tourist_leg(tid, Goal::Sight(AttractionKind::Zoo), sight.pos);
    let mut minds = IdleMind;
    for _ in 0..300 {
        if w.attractions.book(zoo).tourists_today > 0 {
            break;
        }
        w.tick(&mut minds);
    }
    let book = w.attractions.book(zoo);
    assert_eq!(book.tourists_today, 1, "the visitor got there");
    let spent = w.tourism.tourists.get(tid).map_or(0.0, |t| t.spent);
    assert!(spent >= ServiceKind::Zoo.admission(), "and paid {spent:.0}");
    // The day closes: today's figures become yesterday's.
    w.daily_attractions();
    let book = w.attractions.book(zoo);
    assert_eq!((book.tourists_today, book.tourists_yesterday), (0, 1));
}

/// [K10] A zoo and a botanical garden side by side: each one's walks come
/// out to the seam between them (the fences open there) and the walk graph
/// joins the two, so a visitor walks from one to the other inside.
#[test]
fn grounds_side_by_side_join_their_walks() {
    let mut w = town();
    let zoo_lot = home_lot(&w, Vec2::new(-200.0, -30.0));
    let zoo = w.place_building(landmark(ServiceKind::Zoo), w.lots[zoo_lot].center).expect("a zoo");
    finish_all(&mut w);
    let zoo_lot = w.buildings[zoo].lot;
    // The garden on the next plots along, beside the zoo's site.
    let east = w.lots[zoo_lot].polygon.iter().map(|p| p.x).fold(f32::MIN, f32::max);
    let lot = home_lot(&w, Vec2::new(east + 25.0, -30.0));
    let garden = w.place_building(landmark(ServiceKind::BotanicalGarden), w.lots[lot].center).expect("a garden");
    finish_all(&mut w);
    w.implement();
    let garden_lot = w.buildings[garden].lot;
    let seam = super::attractions::grounds_seam(&w.lots[zoo_lot].polygon, &w.lots[garden_lot].polygon).expect("the two sites side by side");
    for l in [zoo_lot, garden_lot] {
        let park = w.lots[l].park.as_ref().expect("grounds walks");
        assert!(park.entrances.iter().any(|e| e.distance(seam) < 4.0), "a way in at the seam {seam}: {:?}", park.entrances);
    }
    // In the walk graph a node of the zoo's walks links straight to the garden's.
    let joined = w.walk.nodes.iter().enumerate().filter(|(_, n)| n.lot == Some(zoo_lot)).any(|(i, _)| w.walk.adj[i].iter().any(|e| w.walk.nodes[e.to as usize].lot == Some(garden_lot) && e.park));
    assert!(joined, "the zoo's walks join the garden's");
}

// ── [r6-architects] the architects' league ──────────────────────────────────

const ARCHITECTS: [ServiceKind; 6] = [ServiceKind::LedgeHouse, ServiceKind::PrairieHall, ServiceKind::HillChapel, ServiceKind::SkySlab, ServiceKind::FanLibrary, ServiceKind::CultureHouse];

#[test]
fn the_architects_landmarks_each_go_down_once_on_a_joined_site() {
    let mut w = town();
    for (i, k) in ARCHITECTS.into_iter().enumerate() {
        assert!(k.is_landmark() && ServiceKind::from_key(k.key()) == Some(k), "{k:?}");
        assert!(!crate::attractions::about(k).is_empty() && k.tourist_pull() > 0.0, "{k:?} says what it is and draws visitors");
        assert!(AttractionKind::for_landmark(k).is_some(), "{k:?} is a sight for tourists");
        let (radius, weight) = k.land_value(Tier::Mid);
        assert!(radius >= 350.0 && weight > 0.2, "{k:?} lifts its quarter");
        // Every name is invented: none borrows an architect's.
        for n in 0..12 {
            let name = k.name(Tier::Mid, n).to_lowercase();
            for real in ["wright", "corbusier", "aalto", "fallingwater", "ronchamp", "unite", "guggenheim", "robie", "viipuri", "baker house", "finlandia"] {
                assert!(!name.contains(real), "{k:?}: {name}");
            }
        }
        let what = landmark(k);
        // Along the north and south blocks in turn, well apart.
        let x = -560.0 + i as f32 * 210.0;
        let lot = home_lot(&w, Vec2::new(x, if i % 2 == 0 { -30.0 } else { 30.0 }));
        assert!(w.placement_problem(what, lot).is_some_and(|p| p.contains("too small")), "{k:?}: a plot alone is too small");
        let id = w.place_building(what, w.lots[lot].center).unwrap_or_else(|e| panic!("{k:?}: {e}"));
        assert!(w.lots[w.buildings[id].lot].area() >= what.min_area() * 0.98, "{k:?} on a joined site");
        assert_eq!(w.landmark(k), Some(id));
        let again = home_lot(&w, Vec2::new(x + 100.0, 30.0));
        let err = w.place_building(what, w.lots[again].center).expect_err("one per city");
        assert!(err.contains("one per city"), "{k:?}: {err}");
    }
    finish_all(&mut w);
    for k in ARCHITECTS {
        let b = &w.buildings[w.landmark(k).unwrap()];
        assert!(b.operating() && b.jobs > 0, "{k:?} opens with staff to hire");
    }
}

#[test]
fn a_ledge_house_lifts_the_land_round_it() {
    let lift = lift(landmark(ServiceKind::LedgeHouse), Vec2::new(0.0, -30.0), 0.0, 140.0);
    assert!(lift > 0.05, "the blocks round it rise: +{lift:.3}");
}

#[test]
fn old_saves_keep_their_kinds_in_order() {
    // The new kinds are appended: every earlier kind keeps its index.
    let idx = |k: ServiceKind| ServiceKind::ALL.iter().position(|x| *x == k).unwrap();
    assert_eq!(idx(ServiceKind::TransferStation), 29);
    assert_eq!(idx(ServiceKind::LedgeHouse), 30);
    assert_eq!(ServiceKind::LANDMARKS[13], ServiceKind::CasinoResort);
    let a = AttractionKind::ALL.iter().position(|x| *x == AttractionKind::CasinoResort).unwrap();
    assert_eq!(a, 20);
}
