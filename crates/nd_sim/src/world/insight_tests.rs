//! [insight] Tests of the city's memory: what is recorded, that it stays
//! bounded, survives a save and load, and that saves from before it load.

use super::*;
use crate::insight::{Ev, Happened, Life, Note, Why, BUILDING_BYTES, DONE_TRIP_BYTES, LIFE_BYTES, PERSON_BYTES, TRIP_BYTES};
use crate::mind::IdleMind;
use crate::vehicle::Errand;

fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
    vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
}

fn run(w: &mut World, ticks: usize) {
    let mut minds = IdleMind;
    for _ in 0..ticks {
        w.tick(&mut minds);
    }
}

/// A main road, a cross street, shops and homes; a day of life.
fn town() -> (World, BuildingId) {
    let mut w = World::new(WorldConfig { emergencies: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
    w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
    w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Commercial && l.car_access()).map(|(id, _)| id).collect();
    w.build_on(lots[0]);
    let shop = w.lots[lots[0]].building.unwrap();
    run(&mut w, 12 * 60);
    let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
    for id in ids {
        w.finish_construction(id);
    }
    run(&mut w, 14 * 60);
    (w, shop)
}

/// Every log within its cap.
fn assert_bounded(w: &World) {
    for t in w.insight.live.values() {
        assert!(t.log.size() <= TRIP_BYTES, "live trip {}", t.log.size());
    }
    for t in &w.insight.done {
        assert!(t.log.size() <= DONE_TRIP_BYTES, "done trip {}", t.log.size());
    }
    assert!(w.insight.done.len() <= crate::insight::DONE_TRIPS);
    for p in w.insight.people.values() {
        assert!(p.trips.size() <= PERSON_BYTES && p.life.size() <= LIFE_BYTES, "person {} / {}", p.trips.size(), p.life.size());
        assert!(p.last_drive.as_ref().is_none_or(|t| t.log.size() <= DONE_TRIP_BYTES));
    }
    for p in w.insight.buildings.values() {
        assert!(p.log.size() <= BUILDING_BYTES, "building {}", p.log.size());
    }
}

#[test]
fn a_day_of_town_life_is_remembered_and_stays_bounded() {
    let (w, shop) = town();
    for p in w.insight.people.values() {
        let mut re = crate::insight::Log::default();
        for r in p.trips.records() { re.push(r, 100_000); }
        assert_eq!(re.size(), p.trips.size(), "decodes whole: {:?}", p.trips.records());
    }
    // People: trips with the mode chosen, legs, and arriving in town.
    let trips = w.insight.people.values().flat_map(|p| p.trips.records()).filter(|r| matches!(r.ev, Ev::Trip { .. })).count();
    let legs = w.insight.people.values().flat_map(|p| p.trips.records()).filter(|r| matches!(r.ev, Ev::Act(_))).count();
    let arrived = w.insight.people.values().flat_map(|p| p.life.records()).filter(|r| matches!(r.ev, Ev::Life { what: Life::Arrived, .. })).count();
    // (A trip home from the doorstep starts and ends in the same tick: no leg.)
    assert!(trips > 0 && legs > 0, "trips {trips}, legs {legs}");
    assert!(arrived > 0, "newcomers noted");
    // The shop: begun and finished.
    let shop_log: Vec<Happened> = w.insight.buildings.get(shop).unwrap().log.records().into_iter().filter_map(|r| if let Ev::Happened { what, .. } = r.ev { Some(what) } else { None }).collect();
    assert!(shop_log.contains(&Happened::Built), "{shop_log:?}");
    assert_bounded(&w);
}

#[test]
fn a_vans_trip_records_its_streets_errands_and_replay() {
    let (mut w, shop) = town();
    let (fs, fp) = w.truck_stop(shop).unwrap();
    let far = w.lots.values().filter(|l| l.car_segment.is_some()).max_by(|a, b| a.center.distance(fp).total_cmp(&b.center.distance(fp))).unwrap();
    let (route, s0, s1) = w.traffic.net.find_route(fs, fp, far.car_segment.unwrap(), far.car_point).expect("route");
    let id = w.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
    w.truck_dispatched(id, Some(shop));
    let mut minds = IdleMind;
    let mut replay_live = 0;
    for _ in 0..900 {
        w.tick(&mut minds);
        if let Some(t) = w.insight.live.get(id) {
            replay_live = replay_live.max(w.trip_replay(t).len());
        }
        if !w.traffic.cars.contains_key(id) {
            break;
        }
    }
    assert!(!w.traffic.cars.contains_key(id), "back at base");
    assert!(replay_live >= 2, "replayed while on the road");
    let t = w.insight.trip_of(id).expect("its trip is kept after it left the road");
    assert!(t.end.is_some());
    let recs = t.log.records();
    assert!(matches!(recs.first().unwrap().ev, Ev::Start) && matches!(recs.last().unwrap().ev, Ev::End(_)));
    assert!(recs.iter().filter(|r| matches!(r.ev, Ev::Enter { .. })).count() >= 2, "street pieces: {recs:?}");
    assert!(recs.iter().any(|r| matches!(r.ev, Ev::Errand { what: 8, .. })), "turned back for base: {recs:?}");
    assert!(recs.iter().any(|r| matches!(r.ev, Ev::Replan { why: Why::Errand, after_m: Some(_), .. })), "the way back was a re-plan for its errand: {recs:?}");
    let pts = w.trip_replay(t);
    assert!(pts.len() >= 3, "{pts:?}");
    assert!(pts.windows(2).all(|p| p[1].tick >= p[0].tick), "times run forward");
}

#[test]
fn u_turns_stalls_and_replans_are_read_from_the_notes_and_the_state() {
    let (mut w, shop) = town();
    let (fs, fp) = w.truck_stop(shop).unwrap();
    let far = w.lots.values().filter(|l| l.car_segment.is_some()).max_by(|a, b| a.center.distance(fp).total_cmp(&b.center.distance(fp))).unwrap();
    let (route, s0, s1) = w.traffic.net.find_route(fs, fp, far.car_segment.unwrap(), far.car_point).expect("route");
    let id = w.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
    w.step_insight();
    let seg = fs.0;
    w.traffic.note(id, Note::Entered { segment: seg, forward: true, at: fp });
    w.traffic.note(id, Note::Entered { segment: seg, forward: false, at: fp });
    w.traffic.note_replan(id, Why::FullExit, None, None);
    // Standing two minutes, then moving on.
    w.traffic.cars[id].stalled = 4.0;
    w.traffic.cars[id].wait = crate::traffic::Wait::ExitFull;
    w.step_insight();
    w.clock.tick += 30;
    w.traffic.cars[id].stalled = 0.0;
    w.traffic.cars[id].drive.anger = 0.45;
    w.traffic.cars[id].errand = Errand::Search { tries: 1 };
    w.step_insight();
    let recs = w.insight.live.get(id).unwrap().log.records();
    assert!(recs.iter().any(|r| matches!(r.ev, Ev::UTurn { .. })), "{recs:?}");
    assert!(recs.iter().any(|r| matches!(r.ev, Ev::Replan { why: Why::FullExit, after_m: None, .. })));
    assert!(recs.iter().any(|r| matches!(r.ev, Ev::Stall { cause: crate::traffic::Wait::ExitFull, seconds, .. } if seconds >= 60)), "{recs:?}");
    assert!(recs.iter().any(|r| matches!(r.ev, Ev::Anger { pct: 45 })));
    assert!(recs.iter().any(|r| matches!(r.ev, Ev::Errand { what: 1, tries: 1, .. })));
}

/// The save's RON with the `insight` field cut out, as a save from before it.
fn without_insight(text: &str) -> String {
    let start = text.find("insight:").expect("saved");
    let q0 = start + text[start..].find('"').unwrap();
    let q1 = q0 + 1 + text[q0 + 1..].find('"').unwrap();
    let rest = text[q1 + 1..].trim_start_matches([',', '\n', '\r', ' ']);
    format!("{}{}", &text[..start], rest)
}

#[test]
fn the_memory_survives_save_and_load_and_old_saves_load_without_it() {
    let (w, _) = town();
    assert!(w.insight.bytes() > 0);
    let text = w.to_ron().unwrap();
    let back = World::from_ron(&text).unwrap();
    assert_eq!(back.insight.encode(), w.insight.encode(), "the same memory after a load");
    // Carries on recording after the load.
    let mut back = back;
    run(&mut back, 120);
    assert_bounded(&back);
    // A save from before the memory.
    let old = without_insight(&text);
    assert!(!old.contains("insight:"));
    let mut w2 = World::from_ron(&old).expect("an old save loads");
    assert!(w2.insight.live.is_empty() && w2.insight.people.is_empty());
    run(&mut w2, 60);
    assert!(!w2.insight.live.is_empty() || w2.traffic.cars.is_empty());
    // The first look after it takes in who is there without calling them newcomers.
    let arrived = w2.insight.people.values().flat_map(|p| p.life.records()).filter(|r| matches!(r.ev, Ev::Life { what: Life::Arrived, .. }) && r.tick <= w.clock.tick + 1).count();
    assert_eq!(arrived, 0);
}
