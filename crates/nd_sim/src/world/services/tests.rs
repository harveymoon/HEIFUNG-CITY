use super::*;
use crate::mind::IdleMind;

fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
}

/// A long avenue with civic land along its north side and housing to
/// the south, everything finished.
fn town() -> World {
    town_with(WorldConfig::default().seed)
}

fn town_with(seed: u64) -> World {
    let mut w = World::new(WorldConfig { seed, ..WorldConfig::test_town() });
    w.economy.treasury = 1_000_000.0;
    w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-700.0, 120.0), Vec2::new(700.0, 120.0)], 1, 1);
    for x in [-600.0, -300.0, 0.0, 300.0, 600.0] {
        w.add_road(&[Vec2::new(x, 0.0), Vec2::new(x, 120.0)], 1, 1);
    }
    w.add_zone(square(-690.0, 8.0, 1380.0, 50.0), LandUse::Civic);
    w.add_zone(square(-690.0, -60.0, 1380.0, 52.0), LandUse::Residential);
    w
}

/// The vacant civic lot nearest `x` that fronts the avenue (y = 0).
fn civic_lot(w: &World, x: f32) -> LotId {
    w.lots
        .iter()
        .filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.building.is_none() && l.car_access() && l.area() >= 420.0 && l.car_point.y.abs() < 12.0)
        .min_by(|a, b| (a.1.center.x - x).abs().total_cmp(&(b.1.center.x - x).abs()))
        .map(|(id, _)| id)
        .expect("a civic lot")
}

/// An ordinary finished building on the civic lot nearest `x`.
fn building_at(w: &mut World, x: f32) -> BuildingId {
    let lot = civic_lot(w, x);
    w.build_on(lot);
    let id = w.lots[lot].building.unwrap();
    w.finish_construction(id);
    id
}

/// Homes and people: housing on the south side, filled by immigration.
fn people(w: &mut World, n: usize) -> Vec<CitizenId> {
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.usable()).map(|(id, _)| id).take(8).collect();
    for l in lots {
        w.build_on(l);
        if let Some(b) = w.lots[l].building {
            w.finish_construction(b);
        }
    }
    for _ in 0..40 {
        if w.citizens.len() >= n {
            break;
        }
        w.immigrate();
    }
    assert!(w.citizens.len() >= n, "only {} citizens", w.citizens.len());
    w.citizens.keys().take(n).collect()
}

/// Put people on a building's payroll, at work if `inside`.
fn staff(w: &mut World, b: BuildingId, who: &[CitizenId], inside: bool) {
    for &c in who {
        if let Some(old) = w.citizens[c].work {
            w.buildings[old].workers.retain(|x| *x != c);
        }
        w.citizens[c].work = Some(b);
        w.buildings[b].workers.push(c);
        if inside {
            let e = w.building_entrance(b).unwrap();
            let cz = &mut w.citizens[c];
            cz.activity = Activity::Working;
            cz.inside = Some(b);
            cz.pos = e;
            cz.next_decision_tick = u64::MAX / 2;
        }
    }
}

fn run_until(w: &mut World, max: usize, mut done: impl FnMut(&World) -> bool) -> usize {
    let mut minds = IdleMind;
    for t in 0..max {
        if done(w) {
            return t;
        }
        w.tick(&mut minds);
    }
    max
}

/// [fire-scene] Once the fire is out the engine stays a few minutes on
/// scene while the crew packs up (still on the job, its incident still
/// named for the renderer), then drives home.
/// [review] Repairs are priced off what a structure cost: batteries and
/// city car parks were priced as cargo hubs.
#[test]
fn repairs_are_priced_off_what_was_built() {
    let mut w = town();
    let bat = w.build_battery(civic_lot(&w, -450.0), crate::power::BatteryKind::Small).unwrap();
    assert_eq!(w.civic_value(bat), Placeable::Battery(crate::power::BatteryKind::Small).cost());
    let park = w.build_city_parking(civic_lot(&w, 450.0), crate::enforcement::LotSize::Small).unwrap();
    assert_eq!(w.civic_value(park), Placeable::ParkingLot(crate::enforcement::LotSize::Small).cost());
    assert_ne!(w.civic_value(park), Placeable::CargoHub.cost());
}

#[test]
fn a_fire_engine_packs_up_before_it_leaves() {
    let mut w = town();
    let crew = people(&mut w, 8);
    let station = w.build_service(civic_lot(&w, -350.0), ServiceKind::Fire, Tier::Mid).unwrap();
    let desk = w.build_service(civic_lot(&w, -150.0), ServiceKind::Dispatch, Tier::Mid).unwrap();
    staff(&mut w, station, &crew[..6], false);
    staff(&mut w, desk, &crew[6..7], true);
    let target = building_at(&mut w, 100.0);
    let id = w.start_fire(target).unwrap();
    run_until(&mut w, 600, |w| !w.services.incident(id).unwrap().active());
    let inc = w.services.incident(id).unwrap();
    assert_eq!(inc.outcome, Some(Outcome::Extinguished));
    let (uid, out_at) = (inc.unit.unwrap(), inc.resolved.unwrap());
    let u = &w.services.units[uid];
    assert!(u.packing(), "packing up: {:?}", u.state);
    assert_eq!(u.incident, Some(id), "still at its fire");
    assert_eq!(u.packing_until, out_at + PACK_UP_TICKS);
    assert!(u.busy() && u.car.is_none(), "parked and not free for calls");
    // Its crew is out with it, and says so.
    let hand = w.unit_crew(uid)[0];
    assert_eq!(w.crew_unit_of(hand), Some(uid));
    assert!(w.activity_line(hand).starts_with("out with Engine"), "{}", w.activity_line(hand));
    run_until(&mut w, 20, |w| !w.services.units[uid].packing());
    let u = &w.services.units[uid];
    assert_eq!(w.clock.tick, out_at + PACK_UP_TICKS, "left on time");
    assert_eq!(u.state, UnitState::Returning);
    assert_eq!(u.packing_until, 0);
    assert!(u.incident.is_none() && u.car.is_some(), "driving home");
}

#[test]
fn a_fire_is_called_in_dispatched_fought_and_put_out() {
    let mut w = town();
    let crew = people(&mut w, 8);
    let station = w.build_service(civic_lot(&w, -350.0), ServiceKind::Fire, Tier::Mid).unwrap();
    let desk = w.build_service(civic_lot(&w, -150.0), ServiceKind::Dispatch, Tier::Mid).unwrap();
    staff(&mut w, station, &crew[..6], false);
    staff(&mut w, desk, &crew[6..7], true);
    assert_eq!(w.crewed_units(station), 2);
    let target = building_at(&mut w, 100.0);
    assert!(w.services.coverage_of(w.buildings[target].lot, ServiceKind::Fire) > 0.5, "the target sits in the station's district");
    let id = w.start_fire(target).unwrap();
    w.drain_events();
    let mut seen = Vec::new();
    let mut minds = IdleMind;
    for _ in 0..600 {
        w.tick(&mut minds);
        for e in w.drain_events() {
            match e {
                SimEvent::EmergencyCall(i) if i == id => seen.push("call"),
                SimEvent::UnitDispatched(i) if i == id => seen.push("dispatch"),
                SimEvent::UnitOnScene(i) if i == id => seen.push("on scene"),
                SimEvent::IncidentResolved(i) if i == id => seen.push("resolved"),
                _ => {}
            }
        }
        if !w.services.incident(id).unwrap().active() {
            break;
        }
        // While it drives, the engine is a car on the road.
        if w.services.incident(id).unwrap().stage == Stage::Dispatched {
            let uid = w.services.incident(id).unwrap().unit.unwrap();
            let car = w.services.units[uid].car.expect("a responding unit drives");
            assert!(w.traffic.cars.contains_key(car));
            assert_eq!(w.services.unit_of_car(car), Some(uid));
        }
    }
    let inc = w.services.incident(id).unwrap();
    assert_eq!(seen, vec!["call", "dispatch", "on scene", "resolved"]);
    assert_eq!(inc.outcome, Some(Outcome::Extinguished));
    assert_eq!(inc.dispatch_at.unwrap() - inc.reported.unwrap(), DISPATCH_ON_SHIFT, "a dispatcher at the desk is quick");
    assert_eq!(inc.dispatcher, Some(crew[6]));
    assert!(w.buildings.contains_key(target), "put out in time");
    assert!(w.buildings[target].condition > BURNED_DOWN);
    assert!(w.citizens[crew[6]].memories.iter().any(|m| m.text.contains("Took a 911 call")), "the dispatcher remembers the job");
    // The engine drives home and parks.
    let uid = inc.unit.unwrap();
    run_until(&mut w, 600, |w| w.services.units[uid].state == UnitState::Idle);
    assert_eq!(w.services.units[uid].state, UnitState::Idle);
    assert!(w.services.units[uid].car.is_none());
    assert!(w.services.stats.responses_today >= 1 && w.services.stats.avg_response() > 0.0);
}

#[test]
fn without_a_dispatcher_the_job_goes_out_slower() {
    let mut w = town();
    let crew = people(&mut w, 8);
    let station = w.build_service(civic_lot(&w, -450.0), ServiceKind::Police, Tier::Mid).unwrap();
    staff(&mut w, station, &crew[..4], false);
    let target = building_at(&mut w, 150.0);
    // No dispatch centre: the regional line.
    assert_eq!(w.dispatcher(), (None, DISPATCH_REGIONAL));
    let slow = w.start_crime(target).unwrap();
    run_until(&mut w, 200, |w| w.services.incident(slow).unwrap().reported.is_some());
    let i = w.services.incident(slow).unwrap();
    let slow_delay = i.dispatch_at.unwrap() - i.reported.unwrap();
    // A centre with staff but nobody at the desk is on call; at the desk is quickest.
    let desk = w.build_service(civic_lot(&w, -150.0), ServiceKind::Dispatch, Tier::Low).unwrap();
    staff(&mut w, desk, &crew[5..6], false);
    assert_eq!(w.dispatcher().1, DISPATCH_ON_CALL);
    staff(&mut w, desk, &crew[6..7], true);
    assert_eq!(w.dispatcher(), (Some(crew[6]), DISPATCH_ON_SHIFT));
    let target2 = building_at(&mut w, 450.0);
    let fast = w.start_crime(target2).unwrap();
    run_until(&mut w, 200, |w| w.services.incident(fast).unwrap().reported.is_some());
    let i = w.services.incident(fast).unwrap();
    let fast_delay = i.dispatch_at.unwrap() - i.reported.unwrap();
    assert!(slow_delay > fast_delay * 5, "regional {slow_delay} vs desk {fast_delay}");
    assert_eq!(slow_delay, DISPATCH_REGIONAL);
    // Both get a car in the end, and each crime is closed one way or another.
    run_until(&mut w, 900, |w| !w.services.incident(slow).unwrap().active() && !w.services.incident(fast).unwrap().active());
    for id in [slow, fast] {
        let o = w.services.incident(id).unwrap().outcome;
        assert!(matches!(o, Some(Outcome::Arrest | Outcome::Escaped)), "{o:?}");
    }
}

#[test]
fn a_service_on_a_built_lot_buys_it_out_but_not_another_service() {
    let mut w = town();
    let folk = people(&mut w, 3);
    let shop = building_at(&mut w, -250.0);
    let lot = w.buildings[shop].lot;
    staff(&mut w, shop, &folk[..1], false);
    let school = w.build_service(lot, ServiceKind::School, Tier::Mid).expect("the city takes the lot");
    assert!(!w.buildings.contains_key(shop), "the shop was pulled down");
    assert_eq!(w.lots[lot].building, Some(school));
    assert_ne!(w.citizens[folk[0]].work, Some(shop), "its staff were laid off");
    assert!(w.drain_events().iter().any(|e| matches!(e, SimEvent::Expropriated(b, l) if *b == shop && *l == lot)));
    let err = w.build_service(lot, ServiceKind::Police, Tier::Low).unwrap_err();
    assert!(err.contains("bulldoze it first"), "{err}");
    assert!(w.buildings.contains_key(school));
}

#[test]
fn coverage_follows_road_distance_and_tier() {
    let mut w = town();
    let lot = civic_lot(&w, -560.0);
    let station = w.build_service(lot, ServiceKind::Fire, Tier::Low).unwrap();
    let r = ServiceKind::Fire.radius(Tier::Low);
    let at = w.lots[lot].car_point;
    // Lots on the avenue: full coverage close by, fading over the last
    // COVERAGE_FADE metres of road, none beyond the radius.
    let mut checked = 0;
    for (id, l) in w.lots.iter() {
        if l.car_segment.is_none() || l.car_point.y.abs() > 12.0 {
            continue;
        }
        let d = l.car_point.distance(at);
        let cov = w.services.coverage_of(id, ServiceKind::Fire);
        if d < r - COVERAGE_FADE - 5.0 {
            assert!(cov > 0.99, "lot {d:.0} m away should be covered: {cov}");
        } else if d > r + 5.0 {
            assert_eq!(cov, 0.0, "lot {d:.0} m away is out of reach");
        } else if d > r - COVERAGE_FADE + 5.0 && d < r - 5.0 {
            let want = (r - d) / COVERAGE_FADE;
            assert!((cov - want).abs() < 0.05, "lot {d:.0} m: {cov} vs {want}");
        }
        checked += 1;
    }
    assert!(checked > 10);
    // Road distance, not straight line: a lot just across the avenue's
    // end block is close as the crow flies but far by road.
    assert_eq!(w.services.coverage_of(lot, ServiceKind::Police), 0.0, "only the kind placed covers");
    // A higher tier reaches further.
    let far = w.lots.iter().filter(|(_, l)| l.car_segment.is_some() && l.car_point.y.abs() < 12.0).map(|(id, l)| (id, l.car_point.distance(at))).filter(|(_, d)| *d > r + 50.0 && *d < ServiceKind::Fire.radius(Tier::High) - COVERAGE_FADE - 50.0).map(|(id, _)| id).next().expect("a lot between the two radii");
    assert_eq!(w.services.coverage_of(far, ServiceKind::Fire), 0.0);
    w.demolish(station);
    w.lots[lot].building = None;
    w.build_service(lot, ServiceKind::Fire, Tier::High).unwrap();
    assert!(w.services.coverage_of(far, ServiceKind::Fire) > 0.99);
    // Coverage cuts the fire risk the hourly roll uses.
    let risk = |cov: f32| FIRE_RISK[1] * (1.0 - FIRE_COVER_CUT * cov);
    assert!(risk(1.0) < risk(0.0) * 0.35);
}

#[test]
fn a_stations_reach_is_its_share_of_the_coverage() {
    let mut w = town();
    let a = w.build_service(civic_lot(&w, -560.0), ServiceKind::Health, Tier::Mid).unwrap();
    // [build-highlight] One station alone: its reach is the coverage.
    let reach = w.service_reach(a);
    assert!(reach.len() > 5);
    for (lot, cov) in &reach {
        assert!((w.services.coverage_of(*lot, ServiceKind::Health) - cov).abs() < 1e-5);
    }
    assert_eq!(reach.len(), w.services.coverage.values().filter(|c| c[2] > 0.0).count());
    assert_eq!(w.placed_as(a), Some(Placeable::Service(ServiceKind::Health, Tier::Mid)));
    // A second one: the coverage is the best of the two, lot by lot.
    let b = w.build_service(civic_lot(&w, 300.0), ServiceKind::Health, Tier::Low).unwrap();
    let rb = w.service_reach(b);
    assert!(!rb.is_empty());
    for (lot, c) in w.services.coverage.iter().filter(|(_, c)| c[2] > 0.0) {
        let best = reach.iter().chain(&rb).filter(|(l, _)| l == lot).map(|(_, v)| *v).fold(0.0f32, f32::max);
        assert!((best - c[2]).abs() < 1e-5);
    }
    // Not a covering kind, or not a placed building: nothing.
    let jail = w.build_service(civic_lot(&w, 0.0), ServiceKind::Jail, Tier::Low).unwrap();
    assert!(w.service_reach(jail).is_empty());
    let grown = w.buildings.iter().find(|(_, b)| b.service.is_none() && b.plant.is_none() && !b.depot && !b.metro && b.rail.is_none() && b.port.is_none()).map(|(id, _)| id);
    if let Some(g) = grown {
        assert_eq!(w.placed_as(g), None);
    }
}

#[test]
fn a_sick_citizen_is_taken_to_hospital() {
    let mut w = town();
    let folk = people(&mut w, 6);
    let hospital = w.build_service(civic_lot(&w, -450.0), ServiceKind::Health, Tier::Mid).unwrap();
    staff(&mut w, hospital, &folk[..4], false);
    assert_eq!(w.services.units.values().filter(|u| u.station == hospital).count(), 2);
    // Somebody at home falls ill.
    let patient = folk[5];
    let home = w.citizens[patient].home.unwrap();
    {
        let e = w.building_entrance(home).unwrap();
        let c = &mut w.citizens[patient];
        c.activity = Activity::AtHome;
        c.inside = Some(home);
        c.pos = e;
    }
    let id = w.fall_ill(patient).unwrap();
    assert!(w.is_patient(patient));
    let mut stages = Vec::new();
    let mut minds = IdleMind;
    for _ in 0..900 {
        w.tick(&mut minds);
        let s = w.services.incident(id).unwrap().stage;
        if s != Stage::Unreported && stages.last() != Some(&s) {
            stages.push(s);
        }
        if s == Stage::Resolved {
            break;
        }
    }
    assert_eq!(stages, vec![Stage::Reported, Stage::Dispatched, Stage::OnScene, Stage::Transporting, Stage::Resolved], "{stages:?}");
    assert_eq!(w.services.incident(id).unwrap().outcome, Some(Outcome::Admitted));
    let c = &w.citizens[patient];
    assert_eq!(c.inside, Some(hospital), "in a bed");
    assert!(matches!(c.activity, Activity::Visiting(h) if h == hospital));
    assert!(c.memories.iter().any(|m| m.text.contains("ambulance took me")));
    assert_eq!(w.services.admissions.len(), 1);
    assert!(w.is_patient(patient), "still a patient while admitted");
    assert!(c.next_decision_tick >= w.clock.tick + STAY_HOURS.0 * 60 - 5);
}

#[test]
fn an_unanswered_fire_spreads_and_burns_down() {
    let mut w = town();
    // A terrace of buildings shoulder to shoulder, no fire station.
    let lots: Vec<LotId> = {
        let mut v: Vec<(f32, LotId)> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.car_access() && l.center.x > 0.0 && l.center.x < 290.0).map(|(id, l)| (l.center.x, id)).collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v.into_iter().map(|(_, id)| id).collect()
    };
    assert!(lots.len() >= 4, "{} lots", lots.len());
    let mut row = Vec::new();
    for &l in &lots {
        w.build_on(l);
        let b = w.lots[l].building.unwrap();
        w.finish_construction(b);
        row.push(b);
    }
    let first = row[lots.len() / 2];
    let id = w.start_fire(first).unwrap();
    let mut spread = 0;
    let mut burned = Vec::new();
    let mut minds = IdleMind;
    for _ in 0..(2 * 1440) {
        w.tick(&mut minds);
        for e in w.drain_events() {
            match e {
                SimEvent::FireSpread(_) => spread += 1,
                SimEvent::BurnedDown(b, _) => burned.push(b),
                _ => {}
            }
        }
    }
    let inc = w.services.incident(id).unwrap();
    assert!(inc.dispatched.is_none(), "there is nobody to send");
    assert_eq!(inc.outcome, Some(Outcome::BurnedDown));
    assert!(burned.contains(&first));
    assert!(!w.buildings.contains_key(first));
    let lot = lots[lots.len() / 2];
    assert_eq!(w.lots[lot].building, None, "the lot is free again");
    assert!(spread > 0, "a fire nobody fights jumps to the neighbours");
    assert!(w.services.stats.burned_down_total >= 1);
    // Rubble keeps development off the lot for a while.
    assert!(w.services.is_rubble(lot));
}

#[test]
fn services_cost_upkeep_and_stadiums_draw_a_paying_crowd() {
    let mut w = town();
    let folk = people(&mut w, 30);
    let big = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.car_access()).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, l)| (id, l.area())).unwrap();
    // A stadium needs room: a small lot is refused with the reason.
    let small = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.car_access() && l.area() < ServiceKind::Stadium.min_area(Tier::High)).map(|(id, _)| id).next().unwrap();
    assert!(w.build_service(small, ServiceKind::Stadium, Tier::High).unwrap_err().contains("too small"));
    assert!(big.1 >= ServiceKind::Stadium.min_area(Tier::Low), "largest civic lot {:.0}", big.1);
    let stadium = w.build_service(big.0, ServiceKind::Stadium, Tier::Low).unwrap();
    let school = w.build_service(civic_lot(&w, 0.0), ServiceKind::School, Tier::Mid).unwrap();
    let _ = school;
    for &c in &folk {
        let h = w.citizens[c].home.unwrap();
        let e = w.building_entrance(h).unwrap();
        let cz = &mut w.citizens[c];
        cz.activity = Activity::AtHome;
        cz.inside = Some(h);
        cz.pos = e;
        cz.savings = 200.0;
    }
    w.services.events_today = vec![stadium];
    let before = w.economy.treasury;
    while w.clock.minute_of_day() != EVENT_MINUTE - 1 {
        w.clock.advance();
    }
    w.tick(&mut IdleMind);
    assert!(w.services.stats.stadium_crowd_today > 0, "fans set off");
    assert!(w.economy.ledger.today_of(Flow::Tickets) > 0.0);
    assert!(w.citizens.values().any(|c| matches!(&c.activity, Activity::Traveling { to: Some(b), .. } | Activity::Driving { to: Some(b), .. } if *b == stadium)));
    let _ = before;
    w.daily_services();
    assert!(w.economy.ledger.today_of(Flow::ServiceUpkeep) < 0.0);
}

#[test]
fn children_go_to_a_school_that_covers_their_home_and_staff_work_round_the_clock() {
    let mut w = town();
    people(&mut w, 40);
    let kids: Vec<CitizenId> = w.citizens.iter().filter(|(_, c)| c.goes_to_school()).map(|(id, _)| id).collect();
    assert!(!kids.is_empty(), "families with school-age children arrived");
    let school = w.build_service(civic_lot(&w, -150.0), ServiceKind::School, Tier::Mid).unwrap();
    for &k in &kids {
        let h = w.citizens[k].home.unwrap();
        let e = w.building_entrance(h).unwrap();
        let c = &mut w.citizens[k];
        c.activity = Activity::AtHome;
        c.inside = Some(h);
        c.pos = e;
    }
    while w.clock.minute_of_day() != SCHOOL_MINUTE - 1 {
        w.clock.advance();
    }
    w.tick(&mut IdleMind);
    assert!(w.services.stats.pupils_today > 0);
    let going = kids.iter().filter(|k| matches!(&w.citizens[**k].activity, Activity::Traveling { to: Some(b), .. } | Activity::Driving { to: Some(b), .. } if *b == school)).count();
    assert!(going > 0, "children set off for school");
    // Emergency services staff in shifts round the clock; schools by day.
    let fire = w.build_service(civic_lot(&w, 350.0), ServiceKind::Fire, Tier::Low).unwrap();
    let shifts: std::collections::HashSet<u8> = (0..30u64).map(|seed| w.buildings[fire].shift_for(seed).open).collect();
    assert!(shifts.len() >= 3, "three shifts: {shifts:?}");
    assert!(!w.buildings[school].opening_hours().contains(22));
}

/// A robbery with a suspect, answered and arrested: with a jail the
/// suspect serves days there; without one they are held a few hours at
/// the station and let go early.
fn arrest_case(with_jail: bool) -> (World, CitizenId, u32) {
    // The arrest is a dice roll: take the first seed where it happens.
    for seed in 1..40 {
        let case = arrest_case_seeded(with_jail, seed);
        if case.0.services.incident(case.2).unwrap().outcome == Some(Outcome::Arrest) {
            return case;
        }
    }
    panic!("no arrest in 40 seeds");
}

fn arrest_case_seeded(with_jail: bool, seed: u64) -> (World, CitizenId, u32) {
    let mut w = town_with(seed);
    let folk = people(&mut w, 8);
    let station = w.build_service(civic_lot(&w, -350.0), ServiceKind::Police, Tier::Mid).unwrap();
    staff(&mut w, station, &folk[..4], false);
    if with_jail {
        w.build_service(civic_lot(&w, -150.0), ServiceKind::Jail, Tier::Low).unwrap();
    }
    let shop = building_at(&mut w, 100.0);
    let suspect = folk[6];
    let id = w.report_crime(CrimeKind::Theft, shop, Some(suspect), Some(folk[7])).unwrap();
    assert_eq!(w.services.incident(id).unwrap().title(), "theft");
    // Make the arrest certain for the test.
    let mut minds = IdleMind;
    for _ in 0..400 {
        w.tick(&mut minds);
        if w.services.incident(id).unwrap().stage == Stage::OnScene {
            w.services.incident_mut(id).unwrap().arrived = Some(w.services.incident(id).unwrap().started);
        }
        if !w.services.incident(id).unwrap().active() {
            break;
        }
    }
    (w, suspect, id)
}

#[test]
fn arrests_go_to_jail_or_are_let_go_early_without_one() {
    let (w, suspect, id) = arrest_case(true);
    let inc = w.services.incident(id).unwrap();
    {
        let c = w.services.custody.iter().find(|c| c.citizen == suspect).expect("in custody");
        assert!(c.jailed && c.until >= w.clock.tick + SENTENCE_DAYS.0 * 1440 - 2);
        assert_eq!(w.services.jail_occupancy().iter().map(|o| o.1).sum::<u32>(), 1);
        assert_eq!(w.services.arrests_of(suspect), 1);
        assert!(inc.people.iter().any(|p| p.role == "suspect" && p.taken_to.as_deref().is_some_and(|t| t.contains("jail"))));
    }
    let (mut w, suspect, id) = arrest_case(false);
    {
        assert_eq!(w.services.incident(id).unwrap().outcome, Some(Outcome::Arrest));
        let c = *w.services.custody.iter().find(|c| c.citizen == suspect).expect("held");
        assert!(!c.jailed && c.until <= w.clock.tick + HELD_HOURS * 60);
        run_until(&mut w, (HELD_HOURS * 60 + 30) as usize, |w| !w.services.in_custody(suspect));
        assert!(!w.services.in_custody(suspect));
        assert_eq!(w.services.stats.released_early_total, 1);
    }
}

#[test]
fn with_no_station_but_a_dispatcher_help_comes_from_out_of_town_for_a_fee() {
    let mut w = town();
    let folk = people(&mut w, 3);
    let desk = w.build_service(civic_lot(&w, -150.0), ServiceKind::Dispatch, Tier::Low).unwrap();
    staff(&mut w, desk, &folk[..1], true);
    let target = building_at(&mut w, 100.0);
    let before = w.economy.treasury;
    let id = w.start_fire(target).unwrap();
    run_until(&mut w, 600, |w| !w.services.incident(id).unwrap().active());
    let inc = w.services.incident(id).unwrap();
    assert!(inc.outside, "an out-of-town engine came");
    assert_eq!(inc.travel_ticks(), Some(OUT_OF_TOWN_TRAVEL));
    assert_eq!(inc.outcome, Some(Outcome::Extinguished));
    assert!(inc.fee > 0.0 && w.economy.treasury <= before - inc.fee);
    assert!(!inc.damage.is_empty() && inc.damage[0].lost > 0.0, "the fire did some damage first");
    assert!(inc.people.iter().any(|p| p.role == "dispatcher"));
}

/// A police post (one car, two officers aboard), a 911 desk with a
/// dispatcher at it, shops along the avenue, and no random incidents.
fn one_car_town() -> (World, BuildingId, Vec<CitizenId>) {
    let mut w = town();
    w.config.emergencies = false;
    w.config.society = false;
    let folk = people(&mut w, 10);
    let station = w.build_service(civic_lot(&w, -600.0), ServiceKind::Police, Tier::Low).unwrap();
    let desk = w.build_service(civic_lot(&w, -450.0), ServiceKind::Dispatch, Tier::Mid).unwrap();
    staff(&mut w, station, &folk[..2], false);
    staff(&mut w, desk, &folk[2..3], true);
    assert_eq!(w.crewed_units(station), 1, "two officers crew one car");
    (w, station, folk)
}

/// A crime called in at once.
fn crime_now(w: &mut World, kind: CrimeKind, at: BuildingId, suspect: Option<CitizenId>) -> u32 {
    let id = w.report_crime(kind, at, suspect, None).unwrap();
    let t = w.clock.tick;
    w.services.incident_mut(id).unwrap().report_at = t;
    id
}

fn stage(w: &World, id: u32) -> Stage {
    w.services.incident(id).unwrap().stage
}

#[test]
fn free_police_patrol_and_answer_from_the_beat() {
    let (mut w, station, _) = one_car_town();
    let shop = building_at(&mut w, 450.0);
    run_until(&mut w, 60, |w| w.services.units.values().any(|u| u.station == station && u.state == UnitState::Patrol));
    let uid = w.services.units.iter().find(|(_, u)| u.station == station && u.kind == UnitKind::PoliceCar).map(|(id, _)| id).unwrap();
    assert_eq!(w.services.units[uid].state, UnitState::Patrol, "a free crewed car goes on patrol");
    assert!(!w.services.units[uid].busy(), "on patrol is free for calls");
    assert_eq!(w.services.stats.on_patrol, 1);
    // Out on the beat (parked or driving), not in the station.
    run_until(&mut w, 200, |w| w.services.units[uid].car.is_none() && w.services.units[uid].state == UnitState::Patrol);
    let beat = w.services.units[uid].pos;
    let home = w.building_lot(station).unwrap().car_point;
    assert!(beat.distance(home) > 60.0, "parked on a beat {beat} away from the station {home}");
    let id = crime_now(&mut w, CrimeKind::Robbery, shop, None);
    run_until(&mut w, 30, |w| stage(w, id) == Stage::Dispatched);
    assert_eq!(w.services.incident(id).unwrap().unit, Some(uid));
    let car = w.services.units[uid].car.expect("driving with lights and sirens");
    assert!(w.traffic.cars[car].pos.distance(beat) < 40.0, "it set off from the beat, not the station");
    run_until(&mut w, 120, |w| stage(w, id) != Stage::Dispatched);
    assert_eq!(stage(&w, id), Stage::OnScene);
    assert_eq!(w.services.stats.response_min_today[IncidentKind::Crime.index()].len(), 1);
}

#[test]
fn queued_calls_go_out_most_urgent_first_the_moment_a_car_clears() {
    let (mut w, _, folk) = one_car_town();
    let shops = [building_at(&mut w, 150.0), building_at(&mut w, 300.0), building_at(&mut w, 450.0)];
    let first = crime_now(&mut w, CrimeKind::Theft, shops[0], None);
    run_until(&mut w, 200, |w| stage(w, first) == Stage::OnScene);
    assert_eq!(stage(&w, first), Stage::OnScene);
    // While the only car is busy on scene: vandalism, then a robbery.
    let minor = crime_now(&mut w, CrimeKind::Vandalism, shops[1], None);
    run_until(&mut w, 2, |_| false);
    let violent = crime_now(&mut w, CrimeKind::Robbery, shops[2], Some(folk[9]));
    run_until(&mut w, 10, |_| false);
    assert_eq!((stage(&w, minor), stage(&w, violent)), (Stage::Reported, Stage::Reported), "both wait in the queue");
    assert_eq!(w.services.stats.queued[IncidentKind::Crime.index()], 2);
    assert!(w.services.stats.saturated_since[IncidentKind::Crime.index()].is_some(), "every police car is busy");
    run_until(&mut w, 200, |w| !w.services.incident(first).unwrap().active());
    let cleared = w.services.incident(first).unwrap().resolved.unwrap();
    run_until(&mut w, 2, |_| false);
    let v = w.services.incident(violent).unwrap();
    assert_eq!(v.stage, Stage::Dispatched, "the robbery goes out first, though called in later");
    assert!(v.dispatched.unwrap() <= cleared + 1, "re-dispatched the moment the car cleared ({} vs {cleared})", v.dispatched.unwrap());
    assert_eq!(stage(&w, minor), Stage::Reported, "the vandalism waits its turn");
    assert!(w.services.stats.queue_max_today[IncidentKind::Crime.index()] >= 2 && w.services.stats.saturated_min_today[IncidentKind::Crime.index()] > 0);
    run_until(&mut w, 400, |w| stage(w, minor) != Stage::Reported);
    assert_eq!(stage(&w, minor), Stage::Dispatched, "and then goes out too");
}

#[test]
fn a_violent_crime_pulls_a_car_off_a_theft() {
    let (mut w, _, _) = one_car_town();
    let near = building_at(&mut w, -300.0);
    let far = building_at(&mut w, 600.0);
    let theft = crime_now(&mut w, CrimeKind::Theft, far, None);
    run_until(&mut w, 20, |w| stage(w, theft) == Stage::Dispatched);
    let uid = w.services.incident(theft).unwrap().unit.unwrap();
    let assault = crime_now(&mut w, CrimeKind::Assault, near, None);
    run_until(&mut w, 10, |w| stage(w, assault) == Stage::Dispatched);
    assert_eq!(w.services.incident(assault).unwrap().unit, Some(uid), "the car on its way to the theft is sent to the assault");
    assert_eq!(stage(&w, theft), Stage::Reported, "the theft goes back in the queue");
    assert_eq!(w.services.stats.diverted_today, 1);
}

#[test]
fn with_every_car_busy_a_long_wait_brings_mutual_aid() {
    let (mut w, _, _) = one_car_town();
    let shops = [building_at(&mut w, 150.0), building_at(&mut w, 450.0)];
    let long = crime_now(&mut w, CrimeKind::Theft, shops[0], None);
    run_until(&mut w, 200, |w| stage(w, long) == Stage::OnScene);
    w.services.incident_mut(long).unwrap().work_left = 1_000.0;
    let before = w.economy.treasury;
    let waiting = crime_now(&mut w, CrimeKind::Robbery, shops[1], None);
    run_until(&mut w, MUTUAL_AID_WAIT as usize - 5, |_| false);
    assert_eq!(stage(&w, waiting), Stage::Reported, "queued at first");
    run_until(&mut w, 20, |w| stage(w, waiting) != Stage::Reported);
    let i = w.services.incident(waiting).unwrap();
    assert!(i.outside && i.unit_name.as_deref().is_some_and(|n| n.contains("mutual aid")), "{:?}", i.unit_name);
    assert_eq!(i.arrive_at, Some(i.dispatched.unwrap() + MUTUAL_AID_TRAVEL));
    assert!(w.economy.treasury < before, "billed");
    assert_eq!(w.services.stats.mutual_aid_today, 1);
}

#[test]
fn crashes_past_the_map_edge_are_the_next_towns_and_a_pile_up_is_one_scene() {
    let (mut w, _, folk) = one_car_town();
    let near = building_at(&mut w, 150.0);
    let far = Vec2::new(crate::landscape::MAP_EDGE + 500.0, 0.0);
    assert!(w.report_crash(far, near, &folk[5..6], &folk[6..7], None, None).is_none(), "out past the edge: not a call of ours");
    assert!(w.services.incidents.is_empty());
    let here = Vec2::new(150.0, 0.0);
    let (a, _) = w.report_crash(here, near, &folk[5..6], &[], None, None).unwrap();
    let (b, _) = w.report_crash(here + Vec2::new(20.0, 0.0), near, &folk[7..8], &[], None, None).unwrap();
    assert_eq!(a, b, "a second crash at the same spot joins the open scene");
}

#[test]
fn crash_scenes_take_as_long_as_the_case_and_a_crash_wave_saturates_the_police() {
    let (mut w, _, folk) = one_car_town();
    let spots: Vec<BuildingId> = [-300.0, -150.0, 0.0, 150.0, 300.0, 450.0].iter().map(|&x| building_at(&mut w, x)).collect();
    // A written-off car alone: a short clear-up; someone hurt: longer; a drunk to arrest: longest.
    let (clear, _) = w.report_crash(Vec2::new(-300.0, 0.0), spots[0], &folk[4..5], &[], None, None).unwrap();
    let (hurt, _) = w.report_crash(Vec2::new(-150.0, 0.0), spots[1], &folk[5..6], &folk[5..6], None, None).unwrap();
    let (drunk, _) = w.report_crash(Vec2::new(0.0, 0.0), spots[2], &folk[6..7], &[], Some(folk[6]), None).unwrap();
    let work = |w: &World, id: u32| w.services.incident(id).unwrap().work_left;
    assert_eq!((work(&w, clear), work(&w, hurt), work(&w, drunk)), (CRASH_WORK_CLEAR, CRASH_WORK, CRASH_WORK_ARREST));
    // A wave of crashes for the one car: every unit busy past the alert threshold.
    let mut k = 0;
    for t in 0..(SATURATED_ALERT_MIN as usize + 60) {
        if t % 15 == 0 {
            let x = [150.0, 300.0, 450.0][k % 3];
            w.report_crash(Vec2::new(x, 0.0), spots[3 + k % 3], &folk[7..8], &folk[7..8], None, None);
            k += 1;
        }
        run_until(&mut w, 1, |_| false);
    }
    let k = IncidentKind::Crash.slot();
    assert!(w.services.stats.saturated_min_today[k] >= SATURATED_ALERT_MIN, "saturated {} min", w.services.stats.saturated_min_today[k]);
    assert!(w.services.stats.queue_max_today[k] >= 2);
}
// ── [services] stations that go, civic fires, through traffic ─────────

/// Is `p` at one of the town's road ends (where a vehicle may leave)?
fn at_road_end(w: &World, p: Vec2) -> bool {
    w.roads.leaf_nodes().into_iter().any(|n| w.roads.node(n).pos.distance(p) < 40.0)
}

#[test]
fn a_station_bulldozed_while_its_car_is_on_a_call_hands_the_call_back_and_drives_off_the_map() {
    let (mut w, station, _) = one_car_town();
    let shop = building_at(&mut w, 450.0);
    let id = crime_now(&mut w, CrimeKind::Robbery, shop, None);
    run_until(&mut w, 60, |w| stage(w, id) == Stage::Dispatched);
    assert_eq!(stage(&w, id), Stage::Dispatched);
    let uid = w.services.incident(id).unwrap().unit.unwrap();
    let car = w.services.units[uid].car.unwrap();
    assert!(!World::is_through_car(&w.traffic.cars[car]), "a police car is not through traffic");
    w.demolish_with(station, None, None);
    assert!(!w.buildings.contains_key(station), "a civic structure comes down at once");
    run_until(&mut w, 12, |_| false);
    let i = w.services.incident(id).unwrap();
    assert!(i.unit.is_none() && matches!(i.stage, Stage::Reported | Stage::Dispatched | Stage::OnScene | Stage::Resolved), "the call went back to the queue: {:?} unit {:?}", i.stage, i.unit);
    assert!(i.unit.is_none_or(|u| w.services.units.get(u).is_some_and(|x| !x.leaving)), "no call names a unit leaving town");
    let u = &w.services.units[uid];
    assert!(u.leaving && !u.lights(), "the car leaves town without lights");
    let mut last = w.traffic.cars[car].pos;
    assert!(!at_road_end(&w, last), "it was still in town when its station went");
    let mut gone_at = None;
    for _ in 0..600 {
        w.tick(&mut IdleMind);
        match w.services.units.get(uid).and_then(|u| u.car).and_then(|c| w.traffic.cars.get(c)) {
            Some(c) => last = c.pos,
            None if !w.services.units.contains_key(uid) => {
                gone_at = Some(last);
                break;
            }
            None => {}
        }
    }
    let at = gone_at.expect("the unit left the world");
    assert!(at_road_end(&w, at), "it left at a road end, not mid-street: {at}");
    // The call was not orphaned: it ends one way or another.
    run_until(&mut w, 3000, |w| !w.services.incident(id).unwrap().active());
    assert!(!w.services.incident(id).unwrap().active(), "the call ends");
}

#[test]
fn a_call_whose_unit_vanished_goes_back_in_the_queue() {
    let (mut w, _, _) = one_car_town();
    let shop = building_at(&mut w, 450.0);
    let id = crime_now(&mut w, CrimeKind::Theft, shop, None);
    run_until(&mut w, 60, |w| stage(w, id) == Stage::Dispatched);
    let uid = w.services.incident(id).unwrap().unit.unwrap();
    // However it happened (a stale save, a bug elsewhere): the unit is gone.
    if let Some(car) = w.services.units.remove(uid).and_then(|u| u.car) {
        w.traffic.remove_car(car);
    }
    run_until(&mut w, 2, |_| false);
    let i = w.services.incident(id).unwrap();
    assert!(i.unit != Some(uid), "the incident no longer names the vanished unit");
    assert!(!i.active() || i.unit.is_none_or(|u| w.services.units.contains_key(u)), "{:?}", i.stage);
}

#[test]
fn a_civic_fire_guts_the_station_and_the_city_repairs_it() {
    let (mut w, station, _) = one_car_town();
    w.config.emergencies = true;
    w.economy.treasury = 50_000.0;
    w.buildings[station].condition = BURNED_DOWN + 0.01;
    let id = w.start_fire(station).unwrap();
    run_until(&mut w, 400, |w| !w.services.incident(id).unwrap().active());
    let i = w.services.incident(id).unwrap();
    assert!(!i.active(), "the fire ends");
    assert!(w.buildings.contains_key(station), "the station still stands");
    assert_eq!(i.outcome, Some(Outcome::Gutted));
    assert!(w.buildings[station].is_derelict() && w.crewed_units(station) == 0, "gutted: out of service");
    assert!(!w.services.rubble.iter().any(|r| Some(r.0) == w.buildings.get(station).map(|b| b.lot)), "no rubble on a standing station");
    let money = w.economy.treasury;
    // Two midnights of repairs bring it back past the derelict line.
    for _ in 0..2 {
        run_until(&mut w, 1500, |w| w.clock.minute_of_day() == 0);
        run_until(&mut w, 1, |_| false);
    }
    assert!(!w.buildings[station].is_derelict(), "repaired: condition {}", w.buildings[station].condition);
    assert!(w.economy.treasury < money, "repairs cost money");
}

#[test]
fn civic_structures_do_not_decay() {
    let (mut w, station, _) = one_car_town();
    let before = w.buildings[station].condition;
    for _ in 0..3 {
        run_until(&mut w, 1500, |w| w.clock.minute_of_day() == 0);
        run_until(&mut w, 1, |_| false);
    }
    assert!(w.buildings[station].condition >= before, "{} -> {}", before, w.buildings[station].condition);
}

/// [fire-scene] The engine pulls in to the kerb on the burning
/// building's side, out of the lanes and clear of the junction, and
/// passing traffic crawls past it, easing out round it, without
/// stopping for good.
#[test]
fn a_fire_engine_parks_at_the_kerb_and_traffic_eases_past() {
    let mut w = town();
    let crew = people(&mut w, 8);
    let station = w.build_service(civic_lot(&w, -350.0), ServiceKind::Fire, Tier::Low).unwrap();
    let desk = w.build_service(civic_lot(&w, -150.0), ServiceKind::Dispatch, Tier::Mid).unwrap();
    staff(&mut w, station, &crew[..3], false);
    staff(&mut w, desk, &crew[6..7], true);
    // Just past the junction at x = 0: its frontage begins at the corner.
    let target = building_at(&mut w, 15.0);
    let id = w.start_fire(target).unwrap();
    run_until(&mut w, 300, |w| w.services.incident(id).unwrap().stage == Stage::OnScene);
    let uid = w.services.incident(id).unwrap().unit.expect("an engine");
    let u = w.services.units[uid].clone();
    assert_eq!(u.state, UnitState::OnScene);
    let (seg, _, q) = w.roads.nearest_street(u.pos).unwrap();
    let half = w.roads.segment(seg).half_width();
    // On the building's side (north, y > 0), its body against the edge
    // of the road: off the lanes but for a little.
    assert!(u.pos.y > 0.0, "on the building's side: {}", u.pos);
    assert!((u.pos.distance(q) - (half - ENGINE_HALF.1 - 0.1)).abs() < 0.3, "at the kerb: {} m off the centre line (half width {half})", u.pos.distance(q));
    assert!(u.heading.cos().abs() > 0.99, "along the street");
    for n in [w.roads.segment(seg).a, w.roads.segment(seg).b] {
        let p = w.roads.node(n).pos;
        assert!(u.pos.distance(p) >= SCENE_JUNCTION_CLEAR, "clear of the junction: {} m", u.pos.distance(p));
    }
    run_until(&mut w, 1, |_| false);
    assert_eq!(w.traffic.scenes.len(), 1, "traffic told about it (set before every traffic step)");
    // A car along the avenue past the engine, in the lane beside it.
    let fwd = Vec2::new(u.heading.cos(), u.heading.sin());
    let lane_y = u.pos.y.signum() * 1.75;
    let from = Vec2::new((u.pos - fwd * 120.0).x, lane_y);
    let to = Vec2::new((u.pos + fwd * 160.0).x, lane_y);
    let (fs, _, _) = w.roads.nearest_street(from).unwrap();
    let (ts, _, _) = w.roads.nearest_street(to).unwrap();
    let (route, s0, s1) = w.traffic.net.find_route(fs, from, ts, to).expect("a route along the avenue");
    let car = w.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 2);
    let (mut passed, mut fastest_by, mut closest, mut dodged) = (false, 0.0f32, f32::MAX, 0.0f32);
    for _ in 0..200 {
        run_until(&mut w, 1, |_| false);
        let Some(c) = w.traffic.cars.get(car) else { break };
        for &(p, _) in &c.drive.poses {
            closest = closest.min(p.distance(u.pos));
        }
        let along = (c.pos - u.pos).dot(fwd);
        // (A tick is a few seconds of driving: look over its length.)
        if along.abs() < 15.0 {
            fastest_by = fastest_by.max(c.v);
            dodged = dodged.max(c.drive.dodge.abs());
        }
        if along > 15.0 {
            passed = true;
            break;
        }
    }
    assert!(passed, "the car got past the engine");
    assert!(fastest_by <= 5.0, "slowed going past: {fastest_by} m/s");
    assert!(dodged > 0.3, "eased out round it: {dodged} m");
    assert!(closest > ENGINE_HALF.1 + 0.6, "never through the engine: centres {closest} m apart");
}

/// [fire-scene] A fire burning hard sounds a second alarm: a second
/// engine with its own crew pulls in behind the first and the two put
/// it out sooner.
#[test]
fn a_big_fire_brings_a_second_engine() {
    let mut w = town();
    let crew = people(&mut w, 8);
    let station = w.build_service(civic_lot(&w, -350.0), ServiceKind::Fire, Tier::Mid).unwrap();
    let desk = w.build_service(civic_lot(&w, -150.0), ServiceKind::Dispatch, Tier::Mid).unwrap();
    staff(&mut w, station, &crew[..6], false);
    staff(&mut w, desk, &crew[6..7], true);
    assert_eq!(w.crewed_units(station), 2);
    let target = building_at(&mut w, 150.0);
    let id = w.start_fire(target).unwrap();
    w.services.incident_mut(id).unwrap().intensity = 0.85;
    let mut both = false;
    let mut apart = 0.0f32;
    for _ in 0..400 {
        run_until(&mut w, 1, |_| false);
        let i = w.services.incident(id).unwrap();
        if !i.active() {
            break;
        }
        let engines: Vec<&Unit> = w.services.units.values().filter(|u| u.incident == Some(id) && u.state == UnitState::OnScene && u.car.is_none()).collect();
        if engines.len() == 2 {
            both = true;
            apart = engines[0].pos.distance(engines[1].pos);
            assert_eq!(w.engines_on_scene(id), 2);
        }
    }
    let i = w.services.incident(id).unwrap();
    assert!(i.second_alarm.is_some(), "the second alarm sounded");
    assert!(i.second.is_some() && i.second != i.unit, "a second engine: {:?} {:?}", i.unit, i.second);
    assert!(both, "both engines at work together");
    assert!(apart >= SECOND_ENGINE_GAP - 1.0, "one behind the other, not on top: {apart} m");
    assert_eq!(w.services.stats.second_alarms_total, 1);
    assert_eq!(i.outcome, Some(Outcome::Extinguished));
    // Both go home afterwards.
    run_until(&mut w, 20, |w| w.services.units.values().all(|u| u.incident != Some(id)));
    assert!(w.services.units.values().all(|u| u.incident != Some(id)), "everyone packed up and left");
}

// ── [murder] ──────────────────────────────────────────────────────────

/// A town with police and a jail, a shop, and a victim with a household
/// of two or more (when immigration gave one) and a friend.
fn murder_town(seed: u64) -> (World, BuildingId, CitizenId, CitizenId, CitizenId) {
    let mut w = town_with(seed);
    let folk = people(&mut w, 16);
    let station = w.build_service(civic_lot(&w, -350.0), ServiceKind::Police, Tier::Mid).unwrap();
    staff(&mut w, station, &folk[..4], false);
    w.build_service(civic_lot(&w, -150.0), ServiceKind::Jail, Tier::Low).unwrap();
    let shop = building_at(&mut w, 100.0);
    let size = |w: &World, c: CitizenId| w.citizens[c].household.and_then(|h| w.households.get(h)).map_or(0, |h| h.members.len());
    let victim = *folk[4..].iter().max_by_key(|&&c| (size(&w, c).min(3), std::cmp::Reverse(c))).unwrap();
    let others: Vec<CitizenId> = folk[4..].iter().copied().filter(|&c| c != victim && w.citizens[c].household != w.citizens[victim].household).collect();
    let (killer, friend) = (others[0], others[1]);
    let tick = w.clock.tick;
    let tie = |w: &mut World, a: CitizenId, b: CitizenId, aff: f32| {
        let t = w.social.circles.entry(a).or_default().tie_or_new(b, tick, crate::social::Met::Around, None);
        t.affinity = aff;
        t.familiarity = 0.9;
    };
    tie(&mut w, killer, victim, -0.95);
    tie(&mut w, victim, killer, -0.95);
    tie(&mut w, victim, friend, 0.9);
    tie(&mut w, friend, victim, 0.9);
    (w, shop, killer, victim, friend)
}

#[test]
fn a_feud_killing_is_grieved_investigated_and_the_known_killer_mostly_caught() {
    assert_eq!(CrimeKind::Homicide.label(), "homicide");
    assert!(CrimeKind::Homicide.violent() && !CrimeKind::ALL.contains(&CrimeKind::Homicide));
    let mut arrested = 0;
    for seed in 1..=5u64 {
        let (mut w, shop, killer, victim, friend) = murder_town(seed);
        let vname = w.citizens[victim].name.clone();
        let family: Vec<CitizenId> = w.citizens[victim].household.and_then(|h| w.households.get(h)).map(|h| h.members.clone()).unwrap_or_default().into_iter().filter(|m| *m != victim).collect();
        let day = w.clock.day();
        let before = w.concerns.count(crate::concerns::Concern::Safety, day, day);
        let id = w.feud_killing(killer, victim, shop).expect("a homicide incident");
        assert!(!w.citizens.contains_key(victim), "the victim is gone from the city");
        assert_eq!(w.society.crime.stats.killed_total, 1);
        assert!(w.citizens[friend].memories.iter().any(|m| m.text.contains(&vname) && m.text.contains("dead")), "the friend grieves");
        for m in &family {
            assert!(w.citizens[*m].memories.iter().any(|x| x.text.contains(&format!("{vname} was killed"))), "the family grieves");
        }
        assert!(w.concerns.count(crate::concerns::Concern::Safety, day, day) > before, "people are afraid");
        assert!(w.social.news.iter().any(|n| n.text.contains(&vname) && n.text.contains("investigating")));
        let inc = w.services.incident(id).unwrap();
        assert_eq!(inc.crime, Some(CrimeKind::Homicide));
        assert!(inc.known_suspect && inc.work_left == HOMICIDE_WORK && inc.suspect == Some(killer));
        // The investigation takes its time, then (mostly) an arrest.
        let mut minds = IdleMind;
        let mut on_scene_from = None;
        for _ in 0..900 {
            w.tick(&mut minds);
            let i = w.services.incident(id).unwrap();
            if i.stage == Stage::OnScene && on_scene_from.is_none() {
                on_scene_from = Some(w.clock.tick);
            }
            if !i.active() {
                break;
            }
        }
        let inc = w.services.incident(id).unwrap().clone();
        assert!(!inc.active(), "seed {seed}: the case closes");
        if inc.outcome == Some(Outcome::Arrest) {
            arrested += 1;
            let worked = inc.resolved.unwrap() - on_scene_from.expect("the police came");
            assert!(worked as f32 >= HOMICIDE_WORK - 2.0, "a long investigation: {worked} min");
            assert!(w.services.in_custody(killer));
            assert!(w.social.news.iter().any(|n| n.text.contains("arrested") && n.text.contains(&vname)));
            let c = w.services.custody.iter().find(|c| c.citizen == killer).unwrap();
            assert!(!c.jailed || c.until >= w.clock.tick + (HOMICIDE_SENTENCE_DAYS.0 - 1) * 1440, "a long sentence");
        }
    }
    assert!(arrested >= 2, "known killers are mostly caught: {arrested} of 5");
}

#[test]
fn an_offenders_job_can_end_in_a_killing_with_no_name_to_go_on() {
    let (mut w, shop, killer, victim, _) = murder_town(7);
    staff(&mut w, shop, &[victim], true);
    let id = w.offend(killer, shop, crate::crime::Offence::Murder).expect("reported");
    assert!(!w.citizens.contains_key(victim));
    let inc = w.services.incident(id).unwrap();
    assert_eq!(inc.crime, Some(CrimeKind::Homicide));
    assert!(!inc.known_suspect);
    assert_eq!(w.society.crime.stats.offences_total[crate::crime::Offence::Murder.index()], 1);
    assert_eq!(w.society.crime.stats.killed_total, 1);
}

#[test]
fn feuds_rarely_turn_deadly_and_always_the_same_way() {
    let n = (0..20_000u64).filter(|&t| super::super::social::feud_turns_deadly(0xABCD, t)).count();
    assert!((300..900).contains(&n), "about 3% of fights: {n}");
    assert_eq!((0..500u64).map(|t| super::super::social::feud_turns_deadly(77, t)).collect::<Vec<_>>(), (0..500u64).map(|t| super::super::social::feud_turns_deadly(77, t)).collect::<Vec<_>>());
}
