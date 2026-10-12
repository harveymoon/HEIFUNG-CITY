//! [waterfront] Lots that run down to the water, the trades that want
//! them, private jetties, and public beaches: access, weather and
//! contamination.

use super::*;
use crate::clock::SimClock;
use crate::landscape::{LandscapeParams, LandscapePreset, WaterKind};
use crate::mind::IdleMind;
use crate::pressure::PressureKind;
use crate::shore::JettyKind;
use crate::zoning::{make_lot, LotShore};

/// Distance from the road's centre line to the shore in the coast town.
const SHORE_GAP: f32 = 75.0;

fn coast_world(seed: u64) -> World {
    let mut w = World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::Coast, seed), ..WorldConfig::test_town() });
    w.economy.treasury = 1_000_000.0;
    w
}

/// A straight stretch of coast near the middle of the map with dry land
/// behind it: a point on the waterline, the unit vector inland and the one
/// along the shore.
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
        // Dry for 220 m inland along 360 m of shore, open sea in front, and
        // the shore straight enough over that stretch.
        let ok = (-4..=4).all(|k| {
            let q = p + along * (k as f32 * 45.0);
            let wet_front = w.landscape.is_water(q - inland * 25.0);
            let dry_back = (1..=11).all(|j| !w.landscape.is_water(q + inland * (j as f32 * 20.0)));
            let shore = w.landscape.nearest_shore(q + inland * 10.0, 40.0).is_some_and(|s| s.water.kind == WaterKind::Sea && s.distance < 25.0);
            wet_front && dry_back && shore
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

/// The land round `at` levelled to a gentle rise inland, so lots by the
/// water are buildable whatever the generator made of the bank.
fn level_shore(w: &mut World, at: Vec2, inland: Vec2) {
    w.landscape.reshape_for_test(move |p, h| {
        let d = p - at;
        if d.length() > 600.0 {
            return h;
        }
        3.5 + 0.01 * d.dot(inland).max(0.0)
    });
}

/// A street along the coast `gap` metres inland, 300 m long. Returns the
/// shore point, inland and along units, and the street's middle.
fn coast_town(seed: u64, gap: f32, reach_water: bool) -> (World, Vec2, Vec2, Vec2, Vec2) {
    let mut w = coast_world(seed);
    w.shore.reach_water = reach_water;
    let (p, inland, along) = coast_spot(&w);
    level_shore(&mut w, p, inland);
    let mid = p + inland * gap;
    w.add_road(&[mid - along * 150.0, mid + along * 150.0], 1, 1);
    (w, p, inland, along, mid)
}

/// A zone on both sides of the coast street, from the water to 90 m inland.
fn zone_both_sides(w: &mut World, p: Vec2, inland: Vec2, along: Vec2, lu: LandUse) {
    let a = p - inland * 30.0 - along * 140.0;
    let b = p - inland * 30.0 + along * 140.0;
    let far = SHORE_GAP + 90.0;
    w.add_zone(vec![a, b, b + inland * (far + 30.0), a + inland * (far + 30.0)], lu);
}

fn seaside(p: Vec2, inland: Vec2, along: Vec2, c: Vec2) -> bool {
    (c - p).dot(inland) < SHORE_GAP && (c - p).dot(along).abs() < 120.0
}

#[test]
fn parcels_between_a_coastal_road_and_the_sea_run_down_to_the_shoreline() {
    let (w, p, inland, along, _) = coast_town(7, SHORE_GAP, true);
    let sea_side: Vec<&crate::zoning::Parcel> = w.parcels.parcels.iter().filter(|q| seaside(p, inland, along, q.center)).collect();
    assert!(sea_side.len() >= 4, "parcels on the sea side of the road: {}", sea_side.len());
    let wet: Vec<&&crate::zoning::Parcel> = sea_side.iter().filter(|q| q.water_frontage >= LotShore::MIN_FRONTAGE).collect();
    assert!(wet.len() * 3 >= sea_side.len() * 2, "most reach the water: {} of {}", wet.len(), sea_side.len());
    for q in &wet {
        assert!(q.path_frontage && q.street_frontage, "a waterfront parcel fronts the street: {:?}", q.center);
        let far = q.polygon.iter().map(|v| (*v - p).dot(inland)).fold(f32::MAX, f32::min);
        assert!(far < 3.0, "it reaches the waterline ({far:.1} m from it)");
    }
    // Cut as they were before (a save from before): the parcels stop at
    // the usual depth and never meet the water.
    let (old, p, inland, along, _) = coast_town(7, SHORE_GAP, false);
    let dry = old.parcels.parcels.iter().filter(|q| seaside(p, inland, along, q.center)).filter(|q| q.water_frontage >= LotShore::MIN_FRONTAGE).count();
    assert_eq!(dry, 0, "without the shore reach nothing touches the water");
}

#[test]
fn waterfront_lots_know_their_frontage_and_carry_a_land_value_premium() {
    let (mut w, p, inland, along, _) = coast_town(7, SHORE_GAP, true);
    zone_both_sides(&mut w, p, inland, along, LandUse::Residential);
    w.update_land_value();
    let (mut front, mut back) = (Vec::new(), Vec::new());
    for (id, l) in w.lots.iter() {
        if (l.center - p).dot(along).abs() > 110.0 || l.landlocked {
            continue;
        }
        if l.shore.is_waterfront() {
            assert!(l.shore.seaward.dot(-inland) > 0.7, "the water lies seaward: {:?}", l.shore.seaward);
            front.push(w.lot_value(id));
        } else if (l.center - p).dot(inland) > SHORE_GAP {
            back.push(w.lot_value(id));
        }
    }
    assert!(front.len() >= 3 && back.len() >= 3, "lots on both sides: {} on the water, {} inland", front.len(), back.len());
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    assert!(mean(&front) > mean(&back) + 0.04, "a premium on the water: {:.3} against {:.3}", mean(&front), mean(&back));
}

#[test]
fn resorts_and_pier_restaurants_open_on_the_water_and_nowhere_else() {
    let (mut w, p, inland, along, _) = coast_town(7, SHORE_GAP, true);
    zone_both_sides(&mut w, p, inland, along, LandUse::Commercial);
    let vacant: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).map(|(id, _)| id).collect();
    assert!(vacant.iter().any(|id| !w.lots[*id].shore.is_waterfront()), "inland lots to choose from too");
    for kind in [PressureKind::BeachResort, PressureKind::PierRestaurant, PressureKind::BoatRental] {
        let lot = w.pressure_lot(kind, &vacant).unwrap_or_else(|| panic!("{kind:?} finds a lot"));
        assert!(w.lots[lot].shore.is_waterfront(), "{kind:?} on the water");
    }
    // Without a lot on the water there is nowhere for them.
    let inland_only: Vec<LotId> = vacant.iter().copied().filter(|id| !w.lots[*id].shore.is_waterfront()).collect();
    assert_eq!(w.pressure_lot(PressureKind::BeachResort, &inland_only), None);
    // By hand, too.
    assert!(w.open_business(inland_only[0], BusinessKind::PierRestaurant, true).is_err());
    let wet = vacant.iter().copied().find(|id| w.lots[*id].shore.is_waterfront()).unwrap();
    let r = w.open_business(wet, BusinessKind::BeachResort, true).expect("a resort on the water");
    assert!(w.hotel_beds(r) > 0, "a resort sleeps visitors");
    assert!(w.night_price_at(r) > crate::tourism::night_price(w.buildings[r].tier), "and charges for the beach");
}

#[test]
fn a_waterfront_pressure_opens_its_trade_and_the_pier_restaurant_builds_its_pier() {
    let (mut w, p, inland, along, _) = coast_town(7, SHORE_GAP, true);
    zone_both_sides(&mut w, p, inland, along, LandUse::Commercial);
    let wet = w.lots.iter().find(|(_, l)| l.shore.is_waterfront() && l.usable() && l.building.is_none()).map(|(id, _)| id).unwrap();
    let r = w.open_business(wet, BusinessKind::PierRestaurant, true).unwrap();
    w.hourly_shore();
    let j = w.shore.jetties.get(&r).expect("a pier over the water");
    assert_eq!(j.kind, JettyKind::Pier);
    assert!(w.landscape.is_water(j.tip) && !w.landscape.is_water(j.root));
}

#[test]
fn a_private_jetty_needs_navigable_water_off_the_lot() {
    // On the sea: a fishing company builds its own jetty and pays for it.
    let (mut w, p, inland, along, _) = coast_town(7, SHORE_GAP, true);
    zone_both_sides(&mut w, p, inland, along, LandUse::Industrial);
    let wet = w.lots.iter().find(|(_, l)| l.shore.is_waterfront() && l.usable() && l.building.is_none()).map(|(id, _)| id).expect("an industrial lot on the water");
    let dry = w.lots.iter().find(|(_, l)| !l.shore.is_waterfront() && l.usable() && l.building.is_none()).map(|(id, _)| id).expect("one inland");
    let f = w.open_business(wet, BusinessKind::Fishery, true).unwrap();
    let g = w.open_business(dry, BusinessKind::Fishery, true).unwrap();
    for b in [f, g] {
        w.buildings[b].business.as_mut().unwrap().money = 5000.0;
    }
    w.hourly_shore();
    let j = w.shore.jetties.get(&f).expect("the fishery on the water built a jetty").clone();
    assert_eq!(j.kind, JettyKind::Fishing);
    assert!(w.landscape.is_water(j.tip) && w.landscape.is_water(j.berth), "out into open water");
    assert!(w.buildings[f].business.as_ref().unwrap().money < 5000.0, "the company paid for it");
    assert!(!w.shore.jetties.contains_key(&g), "inland: no water, no jetty");
    // A works' cargo quay lands its imports at the sea price.
    let wet2 = w.lots.iter().find(|(_, l)| l.shore.is_waterfront() && l.usable() && l.building.is_none()).map(|(id, _)| id).expect("another lot on the water");
    let yard = w.open_business(wet2, BusinessKind::Smelter, true).unwrap();
    w.buildings[yard].business.as_mut().unwrap().money = 50_000.0;
    w.hourly_shore();
    assert_eq!(w.shore.jetties.get(&yard).map(|j| j.kind), Some(JettyKind::Cargo));
    let before = w.buildings[yard].business.as_ref().unwrap().money;
    w.import(yard, Resource::Ore, 4.0, 100.0);
    assert!(w.shore.inbound.iter().any(|(b, _, _)| *b == yard), "the load comes by its own quay");
    assert!(w.buildings[yard].business.as_ref().unwrap().money > before, "at the sea price (a refund on the order)");

    // On a stream: no boat can work it, so no jetty.
    let mut found = false;
    for seed in 1..40u64 {
        let mut s = World::new(WorldConfig { landscape: LandscapeParams::preset(LandscapePreset::Plains, seed), ..WorldConfig::test_town() });
        let Some(stream) = s.landscape.rivers.iter().find(|r| r.kind() == WaterKind::Stream && r.points.len() > 8).cloned() else { continue };
        let k = stream.points.len() / 2;
        let (a, b) = (stream.points[k - 2], stream.points[k + 2]);
        let along = (b - a).normalize_or_zero();
        if along == Vec2::ZERO {
            continue;
        }
        let side = Vec2::new(-along.y, along.x);
        let mid = stream.points[k] + side * (stream.width * 0.5 + 45.0);
        if s.landscape.is_water(mid) {
            continue;
        }
        s.add_road(&[mid - along * 60.0, mid + along * 60.0], 1, 1);
        s.add_zone(vec![mid - along * 60.0 - side * 60.0, mid + along * 60.0 - side * 60.0, mid + along * 60.0 + side * 5.0, mid - along * 60.0 + side * 5.0], LandUse::Industrial);
        let Some(lot) = s.lots.values().find(|l| l.shore.is_waterfront()) else { continue };
        let Some(shore) = s.landscape.nearest_shore(lot.center, 120.0) else { continue };
        if shore.water.kind != WaterKind::Stream {
            continue;
        }
        let why = crate::shore::plan_jetty(lot, &s.landscape, JettyKind::Fishing).expect_err("a stream takes no jetty");
        assert!(why.contains("stream") || why.contains("narrow") || why.contains("too far"), "{why}");
        found = true;
        break;
    }
    assert!(found, "a stream with a lot on it in some map");
}

/// A flat town with a street along y = 0 and, by hand, a lot fronting it
/// (`front`, of `front_use`) and a beach behind it reaching the water.
fn beach_behind(front_use: LandUse) -> (World, LotId, LotId) {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    let zone = w.zones.insert(Zone { polygon: vec![Vec2::new(-10.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(40.0, 90.0), Vec2::new(-10.0, 90.0)], land_use: front_use });
    let a_poly = vec![Vec2::new(0.0, 9.0), Vec2::new(30.0, 9.0), Vec2::new(30.0, 40.0), Vec2::new(0.0, 40.0)];
    let b_poly = vec![Vec2::new(0.0, 40.0), Vec2::new(30.0, 40.0), Vec2::new(30.0, 80.0), Vec2::new(0.0, 80.0)];
    let a = make_lot(zone, front_use, a_poly, Vec2::new(15.0, 24.0), &w.roads).unwrap();
    assert!(!a.landlocked, "the front lot is on the street");
    let mut b = make_lot(zone, LandUse::Park, b_poly, Vec2::new(15.0, 60.0), &w.roads).unwrap();
    b.landlocked = true;
    b.shore = LotShore { frontage: 30.0, seaward: Vec2::Y, beach: true };
    let a = w.lots.insert(a);
    let b = w.lots.insert(b);
    w.rebuild_walk();
    // A hot afternoon.
    w.clock = SimClock::starting_at(0, 14, 0);
    w.weather.temp_c = 30.0;
    w.weather.cloud = 0.1;
    w.hourly_shore();
    (w, a, b)
}

/// Can someone walk from the street to the beach's first towel spot?
fn walk_to_beach(w: &World, beach: LotId) -> bool {
    let spot = w.lots[beach].park.as_ref().and_then(|p| p.benches.first()).map(|b| b.0).expect("towel spots on the sand");
    w.walk.route(Vec2::new(15.0, -8.0), spot).is_some()
}

#[test]
fn a_beach_without_access_is_shut_off_and_one_through_a_park_is_walked_to() {
    let (mut w, front, beach) = beach_behind(LandUse::Commercial);
    let (bid, s) = w.beach_on(beach).expect("the beach has its park building and a name");
    assert!(!s.access, "behind a shop: no way onto it");
    assert!(s.note.as_deref().is_some_and(|n| n.contains("no access")), "{:?}", s.note);
    assert!(!w.park_open_for_visits(beach), "nobody is sent there");
    assert!(!walk_to_beach(&w, beach), "and nobody can walk there");
    assert!(w.buildings[bid].name == s.name && !s.name.is_empty());
    // The shop becomes a park: the beach opens through it.
    w.lots[front].land_use = LandUse::Park;
    w.rebuild_walk();
    w.hourly_shore();
    let (_, s) = w.beach_on(beach).unwrap();
    assert!(s.access && s.open, "{s:?}");
    assert!(w.park_open_for_visits(beach), "people go on a hot afternoon");
    assert!(walk_to_beach(&w, beach), "they can walk to the sand through the park");
}

#[test]
fn a_fouled_beach_closes_with_a_sign_and_opens_when_it_clears() {
    let (mut w, front, beach) = beach_behind(LandUse::Park);
    let _ = front;
    assert!(w.beach_on(beach).unwrap().1.usable());
    let at = w.lots[beach].center;
    w.contamination.add(at, 120.0, 1.0);
    w.hourly_shore();
    let (_, s) = w.beach_on(beach).unwrap();
    assert!(!s.open && !s.usable(), "fouled water closes it");
    assert!(s.note.as_deref().is_some_and(|n| n.contains("closed")), "{:?}", s.note);
    assert_eq!(s.visitors, 0);
    assert!(!w.park_open_for_visits(beach));
    w.contamination = Default::default();
    w.hourly_shore();
    assert!(w.beach_on(beach).unwrap().1.open, "clean again: open");
}

#[test]
fn beach_crowds_follow_the_weather_and_an_arctic_beach_is_empty() {
    let (mut w, p, inland, along, _) = coast_town(7, SHORE_GAP, true);
    // Homes inland, a beach on the water.
    let a = p + inland * (SHORE_GAP + 8.0) - along * 140.0;
    let b = p + inland * (SHORE_GAP + 8.0) + along * 140.0;
    w.add_zone(vec![a, b, b + inland * 120.0, a + inland * 120.0], LandUse::Residential);
    let ids: Vec<usize> = w.parcels.parcels.iter().enumerate().filter(|(_, q)| seaside(p, inland, along, q.center) && q.water_frontage >= LotShore::MIN_FRONTAGE).map(|(i, _)| i).collect();
    assert!(!ids.is_empty());
    let refused: Vec<usize> = w.parcels.parcels.iter().enumerate().filter(|(_, q)| q.water_frontage == 0.0).map(|(i, _)| i).take(3).collect();
    for i in &refused {
        assert!(w.beach_refusal(*i).is_some(), "a beach must touch the water");
    }
    let (_, n) = w.paint_parcels_with(&ids, Some(LandUse::Park), Some(crate::shore::BEACH_SUBZONE), None);
    assert!(n > 0);
    w.implement();
    assert!(w.lots.values().any(|l| l.shore.beach), "beach lots after the implement");
    assert!(w.lots.values().filter(|l| l.shore.beach).all(|l| l.park.as_ref().is_some_and(|p| !p.benches.is_empty())), "laid out as sand with towel spots");
    // Homes built at once, and people moving in.
    let homes: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.usable() && l.building.is_none()).map(|(id, _)| id).collect();
    for lot in homes {
        w.build_on(lot);
        if let Some(b) = w.lots[lot].building {
            w.finish_construction(b);
        }
    }
    let mut minds = IdleMind;
    for _ in 0..3000 {
        if w.citizens.len() >= 12 {
            break;
        }
        w.tick(&mut minds);
    }
    assert!(w.citizens.len() >= 4, "people moved in near the beach: {}", w.citizens.len());
    // The beaches laid out (a park goes up like any building).
    let beaches: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.shore.beach && l.usable()).map(|(id, _)| id).collect();
    for lot in beaches {
        if w.lots[lot].building.is_none() {
            w.build_on(lot);
        }
        if let Some(b) = w.lots[lot].building {
            w.finish_construction(b);
        }
    }
    w.clock = SimClock::starting_at(w.clock.day(), 14, 0);
    w.weather.temp_c = 31.0;
    w.weather.cloud = 0.1;
    w.weather.spell = crate::weather::Spell::Normal;
    w.hourly_shore();
    let hot: f32 = w.shore.beaches.values().map(|s| s.draw).sum();
    assert!(!w.shore.beaches.is_empty() && w.shore.beaches.values().any(|s| s.access), "a reachable beach: {:?}", w.shore.beaches.values().map(|s| &s.note).collect::<Vec<_>>());
    assert!(hot > 0.1, "people on the sand on a hot afternoon: {:?}", w.shore.beaches.values().map(|s| (s.catchment, s.draw)).collect::<Vec<_>>());
    // An Arctic afternoon: nobody.
    w.weather.temp_c = -6.0;
    w.hourly_shore();
    let cold: f32 = w.shore.beaches.values().map(|s| s.draw).sum();
    assert_eq!(w.shore.beaches.values().map(|s| s.visitors).sum::<u32>(), 0);
    assert_eq!(cold, 0.0);
    assert!(w.shore.beaches.values().any(|s| s.note.as_deref().is_some_and(|n| n.contains("cold"))));
    // Rain keeps most away.
    w.weather.temp_c = 31.0;
    w.weather.cloud = 0.95;
    w.hourly_shore();
    let wet: f32 = w.shore.beaches.values().map(|s| s.draw).sum();
    assert!(wet < hot, "fewer in the rain: {wet} against {hot}");
}

/// Cut every `field:` and the value after it out of a RON save.
fn strip_field(text: &str, field: &str) -> String {
    let mut out = text.to_string();
    while let Some(start) = out.find(field) {
        let mut depth = 0i32;
        let mut end = start + field.len();
        for (i, ch) in out[start + field.len()..].char_indices() {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    if depth == 0 {
                        end = start + field.len() + i;
                        break;
                    }
                    depth -= 1;
                    if depth == 0 {
                        end = start + field.len() + i + 1;
                        break;
                    }
                }
                ',' if depth == 0 => {
                    end = start + field.len() + i;
                    break;
                }
                _ => {}
            }
        }
        let rest = out[end..].trim_start_matches([',', '\n', '\r', ' ']).to_string();
        out = format!("{}{}", &out[..start], rest);
    }
    out
}

#[test]
fn saves_round_trip_and_a_save_from_before_loads_with_its_old_parcels() {
    let (mut w, p, inland, along, _) = coast_town(7, SHORE_GAP, true);
    zone_both_sides(&mut w, p, inland, along, LandUse::Commercial);
    let wet = w.lots.iter().filter(|(_, l)| l.shore.is_waterfront()).count();
    assert!(wet > 0);
    let text = w.to_ron().unwrap();
    let back = World::from_ron(&text).unwrap();
    assert!(back.shore.reach_water);
    assert_eq!(back.lots.values().filter(|l| l.shore.is_waterfront()).count(), wet);
    // From before: no shore anywhere in the file.
    let old = strip_field(&text, "shore:");
    assert!(!old.contains("shore:"));
    let back = World::from_ron(&old).expect("an old save loads");
    assert!(!back.shore.reach_water, "an old city keeps the parcels it was cut with");
    assert_eq!(back.lots.len(), w.lots.len(), "every lot is still there");
    assert!(back.shore.beaches.is_empty() && back.shore.jetties.is_empty());
}

#[test]
fn a_road_close_to_the_shore_gets_lots_that_run_down_to_it_not_a_front_row_and_backland() {
    let gap = 58.0;
    let (w, p, inland, along, _) = coast_town(7, gap, true);
    let sea_side: Vec<&crate::zoning::Parcel> = w.parcels.parcels.iter().filter(|q| (q.center - p).dot(inland) < gap && (q.center - p).dot(along).abs() < 120.0).collect();
    assert!(sea_side.len() >= 4, "{}", sea_side.len());
    let wet = sea_side.iter().filter(|q| q.water_frontage >= LotShore::MIN_FRONTAGE && q.street_frontage).count();
    assert!(wet * 4 >= sea_side.len() * 3, "nearly every parcel between road and sea fronts both: {wet} of {}", sea_side.len());
    assert!(sea_side.iter().all(|q| !q.landlocked()), "no backland behind a front row");
}

#[test]
fn a_beach_is_grounds_a_player_path_can_join() {
    let (w, _, beach) = beach_behind(LandUse::Commercial);
    let g = w.grounds().into_iter().find(|g| g.lot == Some(beach)).expect("the beach is listed as grounds");
    assert_eq!(g.kind, crate::walk::GroundsKind::Beach);
    assert!(!g.gates.is_empty(), "with gates on its edge for the Path tool");
}

/// [beaches] (H2) Two public beaches side by side are one beach: one name,
/// one state (its sand together), the sand planned over the seam, one
/// lifeguard's anchor; the lot alone is still its own beach.
#[test]
fn beaches_side_by_side_merge_into_one() {
    let mut w = World::new(WorldConfig::test_town());
    w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    let zone = w.zones.insert(Zone { polygon: vec![Vec2::new(-10.0, 30.0), Vec2::new(200.0, 30.0), Vec2::new(200.0, 90.0), Vec2::new(-10.0, 90.0)], land_use: LandUse::Park });
    let beach = |x0: f32, x1: f32, w: &mut World| {
        let poly = vec![Vec2::new(x0, 40.0), Vec2::new(x1, 40.0), Vec2::new(x1, 80.0), Vec2::new(x0, 80.0)];
        let mut l = make_lot(zone, LandUse::Park, poly, Vec2::new((x0 + x1) * 0.5, 60.0), &w.roads).unwrap();
        l.landlocked = true;
        l.shore = LotShore { frontage: x1 - x0, seaward: Vec2::Y, beach: true };
        w.lots.insert(l)
    };
    let a = beach(0.0, 30.0, &mut w);
    let b = beach(30.0, 60.0, &mut w);
    let lone = beach(120.0, 150.0, &mut w);
    w.rebuild_walk();
    w.hourly_shore();
    assert_eq!(w.shore.beaches.len(), 2, "two beaches: the pair and the lone one");
    let (ba, sa) = w.beach_on(a).expect("a beach");
    let (bb, _) = w.beach_on(b).expect("a beach");
    assert_eq!(ba, bb, "one state for the pair");
    assert_eq!(sa.lots.len(), 2);
    assert!((sa.area - 2400.0).abs() < 50.0, "{}", sa.area);
    let (ia, ib) = (w.lots[a].building.unwrap(), w.lots[b].building.unwrap());
    assert_eq!(w.buildings[ia].name, w.buildings[ib].name, "one name");
    assert_ne!(w.beach_on(lone).unwrap().0, ba);
    // Planned as one: one anchor, the seam shared.
    let (pa, pb) = (w.lots[a].park.as_ref().unwrap(), w.lots[b].park.as_ref().unwrap());
    assert_eq!(pa.cluster_lots, 2);
    assert!(pa.anchor != pb.anchor, "one anchor");
    assert!(!pa.seams.is_empty() && !pb.seams.is_empty());
}
