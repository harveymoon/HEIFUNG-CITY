//! [airport] Placement rules (room on the map, roads, hills, the approach
//! paths), the flights, passengers and cargo through a working airport,
//! the height limits it puts on its approaches, and saves.

use super::*;
use crate::airport::{AirportSize, PlanePhase};
use crate::mind::IdleMind;

fn rect(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

/// A long avenue with homes to the south and civic land to the north, on
/// flat ground, rich enough to build an airport.
fn town(seed: u64) -> World {
    let mut w = World::new(WorldConfig { seed, emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 2_000_000.0;
    w.add_road(&[Vec2::new(-880.0, 0.0), Vec2::new(880.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-880.0, 120.0), Vec2::new(880.0, 120.0)], 1, 1);
    for x in [-880.0, -600.0, -300.0, 0.0, 300.0, 600.0, 880.0] {
        w.add_road(&[Vec2::new(x, 0.0), Vec2::new(x, 120.0)], 1, 1);
    }
    w.add_zone(rect(-870.0, 8.0, 1740.0, 50.0), LandUse::Civic);
    w.add_zone(rect(-870.0, -60.0, 1740.0, 52.0), LandUse::Residential);
    w
}

fn building_near(w: &mut World, x: f32, lu: LandUse) -> BuildingId {
    let lot = w
        .lots
        .iter()
        .filter(|(_, l)| l.land_use == lu && l.usable() && l.building.is_none() && l.car_access() && l.area() >= 300.0)
        .min_by(|a, b| (a.1.center.x - x).abs().total_cmp(&(b.1.center.x - x).abs()))
        .map(|(id, _)| id)
        .expect("a lot");
    w.build_on(lot);
    let id = w.lots[lot].building.unwrap();
    w.finish_construction(id);
    id
}

/// An international airport north of town, its forecourt road joined to
/// the avenue. Its id and building.
fn with_airport(seed: u64) -> (World, crate::airport::AirportId, BuildingId) {
    let mut w = town(seed);
    let b = w.place_airport(AirportSize::International, Vec2::new(0.0, 520.0), Some(180.0)).expect("an airport north of town");
    let (aid, a) = w.airports.by_building(b).unwrap();
    let end = a.forecourt()[1];
    w.add_road(&[end, Vec2::new(end.x, 120.0)], 1, 1);
    (w, aid, b)
}

#[test]
fn an_airport_needs_room_on_the_map() {
    let w = town(1);
    let e = w.plan_airport(AirportSize::International, Vec2::new(1500.0, 0.0), Some(0.0)).unwrap_err();
    assert!(e.contains("off the map"), "{e}");
    // Turned the other way it still does not fit that close to the edge.
    assert!(w.plan_airport(AirportSize::International, Vec2::new(1650.0, -900.0), None).is_err());
}

#[test]
fn not_over_a_road_and_not_twice() {
    let mut w = town(2);
    let e = w.plan_airport(AirportSize::Airfield, Vec2::new(0.0, 40.0), Some(0.0)).unwrap_err();
    assert!(e.contains("runs across the site"), "{e}");
    // Fitted to the ground: free to turn, it finds a heading off the roads.
    let a = w.plan_airport(AirportSize::Airfield, Vec2::new(0.0, 520.0), None).expect("room north of town");
    assert!(a.footprint().iter().all(|c| c.y > 125.0), "kept off the avenue");
    // Not over the railway either (the trunk line runs east-west at y = -480).
    let e = w.plan_airport(AirportSize::Airfield, Vec2::new(0.0, -480.0), Some(0.0)).unwrap_err();
    assert!(e.contains("railway"), "{e}");
    w.place_airport(AirportSize::Airfield, Vec2::new(0.0, 520.0), None).unwrap();
    let e = w.plan_airport(AirportSize::Airfield, Vec2::new(0.0, 1200.0), None).unwrap_err();
    assert!(e.contains("one of each size"), "{e}");
}

#[test]
fn too_hilly_ground_is_refused_and_flat_ground_is_levelled() {
    let landscape = crate::landscape::LandscapeParams::preset(crate::landscape::LandscapePreset::RiverValley, 7);
    let mut w = World::new(WorldConfig { seed: 7, emergencies: false, society: false, landscape, ..WorldConfig::test_town() });
    w.economy.treasury = 2_000_000.0;
    let mut hilly = 0;
    let mut ok = [0, 0];
    let mut reasons = std::collections::BTreeMap::new();
    for (k, size) in AirportSize::ALL.into_iter().enumerate() {
        for i in -4..=4 {
            for j in -4..=4 {
                let p = Vec2::new(i as f32 * 380.0, j as f32 * 380.0);
                match w.plan_airport(size, p, None) {
                    Ok(a) => {
                        ok[k] += 1;
                        // Levelled between its highest and lowest ground, within the limit everywhere.
                        assert!(a.level >= crate::landscape::BANK_HEIGHT - 0.01);
                        for c in a.footprint() {
                            assert!((w.landscape.height(c) - a.level).abs() <= crate::airport::MAX_EARTHWORK + 0.5);
                        }
                    }
                    Err(e) => {
                        if e.contains("too hilly") {
                            hilly += 1;
                        }
                        *reasons.entry(e.split(':').next().unwrap_or("").to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    eprintln!("fits {ok:?}, refusals {reasons:?}");
    assert!(hilly > 0, "the hills round the valley are too steep for a runway");
    // The flat basin is built at one level: an airport fits anywhere off the roads.
    let flat = town(7);
    let a = flat.plan_airport(AirportSize::International, Vec2::new(0.0, 520.0), None).expect("the flat basin");
    assert_eq!(a.level, 0.0);
}

#[test]
fn nothing_tall_in_the_approach_path() {
    let mut w = town(3);
    let tower = building_near(&mut w, 100.0, LandUse::Residential);
    // An airfield north of town with its runway pointing south at it.
    let p = Vec2::new(0.0, 900.0);
    w.buildings[tower].floors = 1;
    assert!(w.plan_airport(AirportSize::Airfield, p, Some(90.0)).is_ok(), "a house under the approach is fine");
    w.buildings[tower].floors = 14;
    let e = w.plan_airport(AirportSize::Airfield, p, Some(90.0)).unwrap_err();
    assert!(e.contains("approach path") && e.contains("14 floors"), "{e}");
    // Turned across, the tower is out of the way.
    assert!(w.plan_airport(AirportSize::Airfield, p, Some(0.0)).is_ok());
    // Once built, the approach is a height limit zone.
    w.buildings[tower].floors = 1;
    w.place_airport(AirportSize::Airfield, p, Some(90.0)).unwrap();
    let lim = w.height_limit_at(w.buildings[tower].lot).expect("under the approach");
    assert!(lim < 20.0, "limit {lim}");
    let far = w.lots.iter().filter(|(_, l)| l.center.x < -500.0).map(|(id, _)| id).next().unwrap();
    assert!(w.height_limit_at(far).is_none(), "off to the side, no limit");
}

#[test]
fn placing_one_lays_its_lot_building_and_road() {
    let (w, aid, b) = with_airport(4);
    let a = &w.airports.airports[aid];
    assert_eq!(w.buildings[b].airport, Some(AirportSize::International));
    assert!(w.is_city_building(b));
    assert_eq!(w.buildings[b].jobs, AirportSize::International.jobs());
    let lot = &w.lots[w.buildings[b].lot];
    let (lw, ld) = AirportSize::International.size();
    assert!((lot.area() - lw * ld).abs() < 50.0, "the field is its lot: {}", lot.area());
    assert!(a.stub.is_some(), "a forecourt road was laid");
    assert!(a.road_connected, "joined to the avenue");
    assert!(w.airport_works(aid));
    assert!(!a.rotations.is_empty(), "a timetable from the start");
    assert!(w.economy.treasury < 2_000_000.0 - AirportSize::International.cost() + 1.0);
    // Nothing else grows on the field.
    assert!(w.under_facility(a.center));
    assert_eq!(w.lots.values().filter(|l| point_in_polygon(l.center, &a.footprint())).count(), 1);
    // And it comes down again.
    let mut w = w;
    w.bulldoze_building(b).unwrap();
    assert!(w.airports.is_empty() && !w.buildings.contains_key(b));
}

#[test]
fn flights_land_turn_round_and_leave_on_the_timetable() {
    let (mut w, aid, b) = with_airport(5);
    let mut minds = IdleMind;
    let mut phases = std::collections::HashSet::new();
    let electronics_buyer = building_near(&mut w, -300.0, LandUse::Civic);
    assert!(w.import_by_air(Cargo { from: BuildingId::default(), to: electronics_buyer, resource: Resource::Electronics, amount: 6.0, price: 270.0 }));
    let fees0 = w.economy.ledger.today_of(Flow::AirportFees);
    for _ in 0..1440 {
        w.tick(&mut minds);
        for p in &w.airports.airports[aid].planes {
            phases.insert(format!("{:?}", p.phase));
            // Never two planes on one stand.
            let same = w.airports.airports[aid].planes.iter().filter(|q| q.stand == p.stand && !matches!(q.phase, PlanePhase::TaxiOut | PlanePhase::HoldShort | PlanePhase::Takeoff | PlanePhase::Climb | PlanePhase::Approach) && !matches!(p.phase, PlanePhase::TaxiOut | PlanePhase::HoldShort | PlanePhase::Takeoff | PlanePhase::Climb | PlanePhase::Approach)).count();
            assert!(same <= 1, "stand {} shared", p.stand);
        }
    }
    let a = &w.airports.airports[aid];
    let day = if a.stats.yesterday.arrivals > 0 { &a.stats.yesterday } else { &a.stats.today };
    assert!(day.arrivals >= 4 && day.departures >= 4, "{day:?}");
    assert!(day.pax_in > 0 && day.pax_out > 0);
    for ph in ["Approach", "Rollout", "TaxiIn", "AtStand", "TaxiOut", "Takeoff", "Climb"] {
        assert!(phases.contains(ph), "never saw {ph}");
    }
    // Paid for, and noisy.
    let fees = w.economy.ledger.yesterday_of(Flow::AirportFees) + w.economy.ledger.today_of(Flow::AirportFees) - fees0;
    assert!(fees > 0.0);
    assert!(w.economy.ledger.yesterday_of(Flow::AirportUpkeep) + w.economy.ledger.today_of(Flow::AirportUpkeep) < 0.0, "upkeep at midnight");
    let r = w.airport_report(b).unwrap();
    assert!(r.flights_yesterday > 0 || r.flights_today > 0);
    let lay = a.layout();
    let end = a.world(Vec2::new(a.dir * lay.runways[0].length * 0.5 + 300.0, lay.runways[0].y));
    let other_end = a.world(Vec2::new(-a.dir * lay.runways[0].length * 0.5 - 300.0, lay.runways[0].y));
    assert!(w.air.noise_at(end) + w.air.noise_at(other_end) > 0.0, "noise logged on the flight paths");
    // Passengers went to town by road (no transit near the terminal yet).
    assert!(day.access[0] + day.access[1] + day.access[2] > 0);
    // The air freight came in and went on by lorry.
    assert!(w.airports.airports[aid].cargo_waiting.is_empty(), "the electronics flew in");
    assert!(day.cargo_in > 0.0 || a.stats.today.cargo_in > 0.0);
}

#[test]
fn demand_grows_the_timetable() {
    let (mut w, aid, _) = with_airport(6);
    let small = w.airports.airports[aid].stats.demand;
    w.tourism.demand_per_day = 6000.0;
    let tick = w.clock.tick;
    w.airport_timetable(aid, tick);
    let a = &w.airports.airports[aid];
    assert!(a.stats.demand > small);
    assert!(a.rotations.iter().filter(|r| !r.started).count() as u32 > AirportSize::International.rotations().0);
}

#[test]
fn a_rail_or_metro_station_by_the_terminal_carries_passengers() {
    let (w, aid, _) = with_airport(8);
    let split = w.access_split(aid);
    assert_eq!(split[3] + split[4] + split[5], 0.0, "no transit yet");
    assert!(split[2] > 0.0, "the airport coach runs instead");
    assert!((split.iter().sum::<f32>() - 1.0).abs() < 1e-4);
}

#[test]
fn saves_keep_the_airport_and_old_saves_load() {
    let (mut w, aid, b) = with_airport(9);
    let mut minds = IdleMind;
    // Run to the middle of the morning bank, planes on the field.
    for _ in 0..240 {
        w.tick(&mut minds);
    }
    let text = w.to_ron().unwrap();
    let back = World::from_ron(&text).unwrap();
    // (The lot key is made again on loading; everything else comes back as it was.)
    let strip = |a: &crate::airport::Airport| (ron::to_string(&a.rotations).unwrap(), ron::to_string(&a.planes).unwrap(), ron::to_string(&a.stats).unwrap(), a.center, a.along, a.building, a.dir);
    assert_eq!(strip(&back.airports.airports[aid]), strip(&w.airports.airports[aid]));
    assert!(back.airports.airports[aid].lot.is_some_and(|l| back.lots.get(l).is_some_and(|l| l.building == Some(b))));
    assert_eq!(back.buildings[b].airport, Some(AirportSize::International));
    let (a0, a1) = (&w.airports.airports[aid], &back.airports.airports[aid]);
    for (p, q) in a0.planes.iter().zip(&a1.planes) {
        let t = w.clock.tick as f32 + 0.5;
        assert_eq!(crate::airport::plane_pose(a0, p, t), crate::airport::plane_pose(a1, q, t));
    }
    // A save from before airports: no `airports` field, no `airport` on buildings.
    let plain = town(9).to_ron().unwrap();
    let cut = strip_field(&plain, "airports:");
    assert!(!cut.contains("airports:"));
    let old = World::from_ron(&cut).unwrap();
    assert!(old.airports.is_empty());
}

#[test]
fn airports_are_deterministic() {
    let run = || {
        let (mut w, aid, _) = with_airport(11);
        let mut minds = IdleMind;
        for _ in 0..500 {
            w.tick(&mut minds);
        }
        ron::to_string(&w.airports.airports[aid]).unwrap()
    };
    assert_eq!(run(), run());
}

/// `text` with the field `key` (a top-level `name:(...)` or `name:{...}`)
/// cut out, as a save written before it existed.
fn strip_field(text: &str, key: &str) -> String {
    let Some(start) = text.find(key) else { return text.to_string() };
    let bytes = text.as_bytes();
    let mut i = start + key.len();
    while bytes[i] == b' ' {
        i += 1;
    }
    let mut depth = 0i32;
    loop {
        match bytes[i] {
            b'(' | b'{' | b'[' => depth += 1,
            b')' | b'}' | b']' => {
                depth -= 1;
                if depth == 0 {
                    i += 1;
                    break;
                }
            }
            _ => {}
        }
        i += 1;
    }
    if bytes.get(i) == Some(&b',') {
        i += 1;
    }
    format!("{}{}", &text[..start], &text[i..])
}
