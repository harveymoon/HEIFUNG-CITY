//! [rail-crossing] Level crossings: the arms come down before a train and
//! go up after it, vehicles wait at the stop line and go on after, small
//! streets get lights only, it all runs the same twice, and saves load.

use super::*;
use crate::level_crossing::{CrossingGuard, CrossingPhase, LevelCrossings};
use crate::mind::IdleMind;
use crate::rail::{Train, TrainKind, TRUNK_Y};
use crate::traffic::{CarId, LaneKind, Wait};

fn tick(w: &mut World, n: u32) {
    let mut minds = IdleMind;
    for _ in 0..n {
        w.tick(&mut minds);
    }
}

/// A plain town with a street of `fwd`+`bwd` lanes over the trunk line at
/// x = 0, and the crossing it makes.
fn town(fwd: u8, bwd: u8) -> (World, usize) {
    let mut w = World::new(WorldConfig::test_town());
    w.economy.treasury = 1_000_000.0;
    w.add_road(&[Vec2::new(0.0, TRUNK_Y - 300.0), Vec2::new(0.0, TRUNK_Y + 300.0)], fwd, bwd);
    tick(&mut w, 1);
    let k = w.level_crossings().iter().position(|c| c.pos.distance(Vec2::new(0.0, TRUNK_Y)) < 8.0).expect("a level crossing where the street meets the trunk line");
    (w, k)
}

/// A passenger train running east along the track through crossing `k`,
/// its head `before` metres short of the crossing's centre.
fn send_train(w: &mut World, k: usize, before: f32) -> crate::rail::TrainId {
    let c = &w.level_crossings()[k];
    let d = if c.rail_dir.x >= 0.0 { c.rail_dir } else { -c.rail_dir };
    let poly = vec![c.pos - d * 1500.0, c.pos + d * 1500.0];
    let s = 1500.0 - before;
    w.rail.trains.insert(Train { kind: TrainKind::Passenger, cars: 3, nodes: Vec::new(), poly, s, prev_s: s, speed: rail::PASSENGER_SPEED, stops: Vec::new(), next_stop: 0, dwell: 0.0, passengers: Vec::new(), cargo: Vec::new(), color: 0, held: false, branch: None, exports: 0.0, unloaded: 0.0 })
}

/// A car driving north over the crossing from `south` metres south of it.
fn send_car(w: &mut World, south: f32) -> CarId {
    let from = Vec2::new(0.0, TRUNK_Y - south);
    let to = Vec2::new(0.0, TRUNK_Y + 250.0);
    let (fs, _, fq) = w.roads.nearest_where(from, |s| s.is_street()).unwrap();
    let (ts, _, tq) = w.roads.nearest_where(to, |s| s.is_street()).unwrap();
    let (route, s0, s1) = w.traffic.net.find_route(fs, fq, ts, tq).expect("a route over the crossing");
    w.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 0)
}

/// The crossing's box: its connector lanes.
fn box_lanes(w: &World, k: usize) -> Vec<u32> {
    let node = w.level_crossings()[k].node;
    w.traffic.net.lanes.iter().enumerate().filter(|(_, l)| matches!(l.kind, LaneKind::Connector { node: n, .. } if n == node)).map(|(i, _)| i as u32).collect()
}

/// Where the train's head and tail are against the crossing's box, along
/// the track: (head past the near edge, tail past the far edge).
fn train_at(w: &World, id: crate::rail::TrainId, k: usize) -> Option<(bool, bool)> {
    let t = w.rail.trains.get(id)?;
    let c = &w.level_crossings()[k];
    let (s, _) = crate::rail::project_on_poly(&t.poly, c.pos)?;
    Some((t.s >= s - c.reach, t.s - t.length() > s + c.reach))
}

#[test]
fn a_wide_street_gets_barriers_and_a_two_lane_one_lights_only() {
    let (w, k) = town(2, 2);
    let c = &w.level_crossings()[k];
    assert_eq!(c.guard, CrossingGuard::Barriers, "2+2 lanes: half-barriers");
    assert_eq!(c.arms.len(), 2);
    assert!(c.arms.iter().all(|a| a.lanes_in == 2 && a.stop > crate::roads::RAIL_HALF_WIDTH + 2.0), "{:?}", c.arms);
    let (w, k) = town(1, 1);
    assert_eq!(w.level_crossings()[k].guard, CrossingGuard::Lights, "1+1 lanes: lights and the bell, no barrier");
    assert!(!w.level_crossings()[k].guard.has_arms());
    let (w, k) = town(2, 1);
    assert_eq!(w.level_crossings()[k].guard, CrossingGuard::Barriers, "three lanes in all is more than two");
}

#[test]
fn the_lanes_stop_short_of_the_track() {
    let (w, k) = town(2, 2);
    let c = w.level_crossings()[k].clone();
    // Every lane arriving at the crossing ends at its stop line, clear of
    // the track's bed.
    for l in &w.traffic.net.lanes {
        if let LaneKind::Road { .. } = l.kind {
            if l.end_node == c.node {
                let from_track = (l.end() - c.pos).dot(Vec2::new(-c.rail_dir.y, c.rail_dir.x)).abs();
                assert!(from_track > crate::roads::RAIL_HALF_WIDTH + 2.5, "lane ends {from_track:.1} m from the track centre");
            }
        }
    }
}

#[test]
fn barriers_come_down_before_the_train_and_go_up_after_it_clears() {
    let (mut w, k) = town(2, 2);
    let train = send_train(&mut w, k, 900.0);
    let mut log = Vec::new();
    let mut was_over = false;
    let mut cleared_tick = None;
    for _ in 0..40 {
        tick(&mut w, 1);
        let c = &w.level_crossings()[k];
        let at = train_at(&w, train, k);
        log.push((c.phase, c.arm, at));
        if let Some((head_in, tail_out)) = at {
            if head_in && !tail_out {
                was_over = true;
                assert_eq!(c.phase, CrossingPhase::Closed, "the train is on the crossing only with the arms down: {log:?}");
                assert_eq!(c.arm, 1.0);
            }
            if tail_out && cleared_tick.is_none() {
                cleared_tick = Some(w.clock.tick);
            }
        }
        if cleared_tick.is_some_and(|t| w.clock.tick > t + 3) {
            break;
        }
    }
    assert!(was_over, "the train went over: {log:?}");
    let c = &w.level_crossings()[k];
    assert_eq!(c.phase, CrossingPhase::Open, "up again once the train has gone: {log:?}");
    assert_eq!(c.arm, 0.0);
    assert!(log.iter().any(|x| x.0 == CrossingPhase::Warning || x.0 == CrossingPhase::Lowering), "lights first, then the arms: {log:?}");
    assert_eq!(c.trains_total, 1);
    assert_eq!(c.closures, 1);
    assert_eq!(c.trains_held, 0, "the arms were down in time: the train never waited");
    assert!(c.trains_last_hour(w.clock.tick) == 1);
}

/// One run of a car meeting a train at the crossing: per tick, the phase,
/// the car's position and why it waits. Panics if the car is ever in the
/// box while the train is over it.
fn car_meets_train() -> (Vec<(CrossingPhase, Option<(Vec2, Wait)>)>, World, usize) {
    let (mut w, k) = town(2, 2);
    let boxed = box_lanes(&w, k);
    let car = send_car(&mut w, 200.0);
    let train = send_train(&mut w, k, 420.0);
    let mut log = Vec::new();
    for _ in 0..30 {
        tick(&mut w, 1);
        let c = &w.level_crossings()[k];
        let over = train_at(&w, train, k).is_some_and(|(i, o)| i && !o);
        let car_now = w.traffic.cars.get(car).map(|x| (x.pos, x.wait));
        if over {
            for &l in &boxed {
                assert!(w.traffic.net.lanes[l as usize].cars.is_empty(), "no vehicle on the crossing with the train over it");
            }
        }
        log.push((c.phase, car_now));
    }
    (log, w, k)
}

#[test]
fn cars_stop_at_the_line_wait_for_the_train_and_go_on_after() {
    let (log, w, k) = car_meets_train();
    let waited = log.iter().any(|(_, c)| c.is_some_and(|(_, why)| why == Wait::RailCrossing));
    assert!(waited, "the car stopped for the crossing: {log:?}");
    // Stopped south of the track, short of the stop line.
    let stop = w.level_crossings()[k].arms.iter().map(|a| a.stop).fold(0.0, f32::max);
    for (_, c) in &log {
        if let Some((p, Wait::RailCrossing)) = c {
            assert!(p.y < TRUNK_Y - crate::roads::RAIL_HALF_WIDTH - 1.0, "waits clear of the track ({:.1} m from it, stop line at {stop:.1})", TRUNK_Y - p.y);
        }
    }
    // And went on over once the crossing opened.
    let crossed = log.iter().any(|(ph, c)| *ph == CrossingPhase::Open && c.is_none_or(|(p, _)| p.y > TRUNK_Y + 10.0));
    assert!(crossed, "drove on over after the train: {log:?}");
    let c = &w.level_crossings()[k];
    assert!(c.vehicles_held >= 1 && c.average_wait() > 0.0, "counted: {} held, {:.1} s average", c.vehicles_held, c.average_wait());
}

#[test]
fn level_crossings_are_deterministic() {
    let (a, wa, _) = car_meets_train();
    let (b, wb, _) = car_meets_train();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    assert_eq!(wa.clock.tick, wb.clock.tick);
}

#[test]
fn walkers_wait_at_a_closed_crossing() {
    let (mut w, k) = town(1, 1);
    let train = send_train(&mut w, k, 300.0);
    tick(&mut w, 1);
    assert!(w.level_crossings()[k].shut(), "lights on");
    let c = w.level_crossings()[k].clone();
    let n = Vec2::new(-c.rail_dir.y, c.rail_dir.x);
    // Walking over the track along the pavement: stopped at the band.
    let a = c.pos - n * 10.0 + c.rail_dir * (c.arms[0].half_width + 1.5);
    let b = c.pos + n * 10.0 + c.rail_dir * (c.arms[0].half_width + 1.5);
    let (_, stop) = w.rail_stop(a, b).expect("stopped short of the track");
    assert!((stop - c.pos).dot(n).abs() >= crate::level_crossing::WALK_BAND, "at the edge of the band");
    // One already on it walks off.
    assert!(w.rail_stop(c.pos - n * 0.5, b).is_none());
    let _ = train;
}

#[test]
fn the_crossing_state_round_trips_and_old_saves_load() {
    let (mut w, k) = town(2, 2);
    send_train(&mut w, k, 300.0);
    tick(&mut w, 2);
    assert!(w.level_crossings()[k].shut());
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(back.level_crossings.list.len(), w.level_crossings.list.len());
    assert_eq!(back.level_crossings.list[k].phase, w.level_crossings.list[k].phase);
    assert_eq!(back.level_crossings.list[k].closures, 1);
    // A save from before level crossings, or with a crossing missing
    // fields, loads with the defaults.
    let old: LevelCrossings = ron::from_str("()").unwrap();
    assert!(old.list.is_empty());
    let partial: LevelCrossings = ron::from_str("(list: [(phase: Closed, closures: 3)])").unwrap();
    assert_eq!(partial.list[0].phase, CrossingPhase::Closed);
    assert_eq!(partial.list[0].arm, 0.0);
    // The trains are not saved: after a load the crossing opens.
    let mut back = back;
    tick(&mut back, 6);
    let c = &back.level_crossings()[k];
    assert_eq!(c.phase, CrossingPhase::Open, "no train after a load: open again ({:?})", c.phase);
}
