//! [r4] Trains on the trunk line: a train behind another brakes and keeps
//! its distance (K50); a freight train draws up along the yard, not with
//! its first wagon at it (K33), and not standing on a level crossing
//! while it is loaded (K51); its imports ride in standard containers that
//! come off at their yard and exports go on (K33).

use super::*;
use crate::mind::IdleMind;
use crate::rail::{Lift, Train, TrainKind, TRUNK_Y};

fn tick(w: &mut World, n: u32) {
    let mut minds = IdleMind;
    for _ in 0..n {
        w.tick(&mut minds);
    }
}

fn train(kind: TrainKind, cars: u8, poly: Vec<Vec2>, s: f32, speed: f32) -> Train {
    Train { kind, cars, nodes: Vec::new(), poly, s, prev_s: s, speed, stops: Vec::new(), next_stop: 0, dwell: 0.0, passengers: Vec::new(), cargo: Vec::new(), color: 0, held: false, branch: None, exports: 0.0, unloaded: 0.0, v: speed, dwell_total: 0.0, boxes: Vec::new() }
}

/// (K50) A passenger train running up behind a freight train standing on
/// the line slows down well before it and comes to a stand a safe gap
/// behind its tail: it never runs into it, never stops dead from speed,
/// and goes on again once the freight moves off.
#[test]
fn a_train_behind_another_brakes_and_keeps_its_distance() {
    let mut w = World::new(WorldConfig::test_town());
    let line = vec![Vec2::new(-3000.0, TRUNK_Y + 900.0), Vec2::new(3000.0, TRUNK_Y + 900.0)];
    let mut freight = train(TrainKind::Freight, 10, line.clone(), 2000.0, rail::FREIGHT_SPEED);
    freight.dwell = 40_000.0;
    freight.v = 0.0;
    let fid = w.rail.trains.insert(freight);
    let pid = w.rail.trains.insert(train(TrainKind::Passenger, 4, line, 200.0, rail::PASSENGER_SPEED));
    let tail = |w: &World| w.rail.trains[fid].s - w.rail.trains[fid].length();
    let mut last_v = rail::PASSENGER_SPEED;
    let mut min_gap = f32::MAX;
    for _ in 0..200 {
        tick(&mut w, 1);
        let p = &w.rail.trains[pid];
        let gap = tail(&w) - p.s;
        min_gap = min_gap.min(gap);
        assert!(gap >= rail::SAFE_GAP - 0.5, "ran up to {gap:.1} m behind the freight");
        let drop = last_v - p.v;
        assert!(drop <= rail::BRAKE * w.config.traffic_seconds_per_tick * 1.6 + 0.5, "braked from {last_v:.1} to {:.1} m/s in one tick", p.v);
        last_v = p.v;
    }
    assert!(min_gap < rail::SAFE_GAP + 5.0, "it closed up behind the freight: {min_gap:.1} m");
    assert!(w.rail.trains[pid].v < 0.1, "and stands there");
    assert!(matches!(w.rail.state(pid), Some(crate::rail::TrainState::HeldBlock { by: Some(b) }) if b == fid));
    // The freight moves off: the passenger train follows.
    w.rail.trains[fid].dwell = 0.0;
    let at = w.rail.trains[pid].s;
    tick(&mut w, 30);
    assert!(w.rail.trains[pid].s > at + 100.0, "it went on after the freight");
}

/// A town with a working freight yard beside the trunk line at x = 0 and,
/// when `crossing_at` is given, a street over the track there.
fn yard_town(crossing_at: Option<f32>) -> (World, BuildingId) {
    let mut w = World::new(WorldConfig::test_town());
    w.economy.treasury = 1_000_000.0;
    w.add_road(&[Vec2::new(-500.0, TRUNK_Y + 120.0), Vec2::new(500.0, TRUNK_Y + 120.0)], 1, 1);
    if let Some(x) = crossing_at {
        w.add_road(&[Vec2::new(x, TRUNK_Y - 200.0), Vec2::new(x, TRUNK_Y + 120.0)], 1, 1);
    }
    let yard = w.place_rail_facility(RailFacility::FreightYard, Vec2::new(0.0, TRUNK_Y + 40.0)).expect("a yard by the trunk");
    let end = w.facilities.values().find(|f| f.building == Some(yard)).unwrap().road_line()[1];
    w.add_road(&[end, Vec2::new(end.x, TRUNK_Y + 120.0)], 1, 1);
    // A city at the other end of the street.
    w.add_zone(vec![Vec2::new(-300.0, TRUNK_Y + 130.0), Vec2::new(300.0, TRUNK_Y + 130.0), Vec2::new(300.0, TRUNK_Y + 220.0), Vec2::new(-300.0, TRUNK_Y + 220.0)], LandUse::Residential);
    tick(&mut w, 2);
    let f = w.facilities.values().find(|f| f.building == Some(yard)).unwrap();
    assert!(w.buildings[yard].rail.as_ref().unwrap().works(), "the yard works: {:?} at {:?} along {:?}, road line {:?}", w.buildings[yard].rail, f.center, f.along, f.road_line());
    (w, yard)
}

/// Arc lengths of the platform's two ends along `t`'s route.
fn platform_on(w: &World, t: &Train, yard: BuildingId) -> (f32, f32) {
    let l = w.buildings[yard].rail.as_ref().unwrap();
    let a = crate::rail::project_on_poly(&t.poly, l.platform[0]).unwrap().0;
    let b = crate::rail::project_on_poly(&t.poly, l.platform[1]).unwrap().0;
    (a.min(b), a.max(b))
}

/// (K33) A long freight train draws up with its head at the far end of the
/// yard's apron, its wagons along it, not with only its first wagon there.
#[test]
fn a_freight_train_draws_up_along_the_yard() {
    let (mut w, yard) = yard_town(None);
    assert!(w.spawn_train(TrainKind::Freight, &[yard]));
    let t = w.rail.trains.values().next().unwrap().clone();
    let (p0, p1) = platform_on(&w, &t, yard);
    let head = t.stops[0].1;
    assert!(t.length() > p1 - p0, "the test train is longer than the apron");
    assert!((head - p1).abs() < 1.0, "head at the apron's far end: {head:.1} against {p0:.1}..{p1:.1}");
    assert!(head - t.length() < p0, "and its wagons the whole length of it");
}

/// (K51) A freight yard right beside a street over the track: the train
/// does not stand over the crossing while it is loaded, so the street is
/// shut only while it passes, not for the whole stand.
#[test]
fn a_freight_train_loading_does_not_stand_on_a_level_crossing() {
    // The street crosses just behind the apron's near end.
    let (mut w, yard) = yard_town(Some(-70.0));
    assert!(!w.level_crossings().is_empty(), "the street makes a level crossing");
    assert!(w.spawn_train(TrainKind::Freight, &[yard]));
    let id = w.rail.trains.keys().next().unwrap();
    let t = w.rail.trains[id].clone();
    let head = t.stops[0].1;
    let k = w.level_crossings().iter().position(|c| c.pos.x.abs() < 100.0 && (c.pos.y - TRUNK_Y).abs() < 5.0).expect("the crossing");
    let c = w.level_crossings()[k].clone();
    let sc = crate::rail::project_on_poly(&t.poly, c.pos).unwrap().0;
    assert!(head < sc - c.reach || head - t.length() > sc + c.reach, "stands clear of the crossing: head {head:.0}, tail {:.0}, crossing {sc:.0}", head - t.length());
    // Run it to the yard and through its stand: the crossing opens while it loads.
    let mut open_while_loading = 0;
    for _ in 0..300 {
        tick(&mut w, 1);
        let Some(t) = w.rail.trains.get(id) else { break };
        if t.dwelling_at() == Some(yard) && !w.level_crossings()[k].shut() {
            open_while_loading += 1;
        }
    }
    assert!(open_while_loading > 3, "the street was open while the train loaded ({open_while_loading} ticks)");
}

/// (K33) Imports ride in standard boxes, wagon by wagon; at their yard the
/// crane lifts them off and the exports on, the stand lasting as long as
/// the moves take; then the train leaves with the exports aboard.
#[test]
fn freight_boxes_come_off_at_their_yard_and_exports_go_on() {
    let (mut w, yard) = yard_town(None);
    let buyer = BuildingId::default();
    w.rail.orders.push(crate::rail::RailOrder { cargo: Cargo { from: yard, to: buyer, resource: Resource::Meat, amount: 60.0, price: 10.0 }, since: 0 });
    w.rail.orders.push(crate::rail::RailOrder { cargo: Cargo { from: yard, to: buyer, resource: Resource::Fuel, amount: 20.0, price: 10.0 }, since: 0 });
    w.buildings[yard].rail_outbound = 70.0;
    assert!(w.spawn_train(TrainKind::Freight, &[yard]));
    let id = w.rail.trains.keys().next().unwrap();
    {
        let t = &w.rail.trains[id];
        // 60 units: 3 TEU (a forty and a twenty reefer); 20 of fuel: a tank.
        assert_eq!(t.boxes.len(), 3, "{:?}", t.boxes);
        assert!(t.boxes.iter().filter(|b| b.resource == Some(Resource::Meat)).all(|b| b.kind == crate::rail::BoxKind::Reefer));
        assert!(t.boxes.iter().any(|b| b.kind == crate::rail::BoxKind::Tank && !b.forty));
        assert!(t.boxes.iter().all(|b| b.wagon >= 1 && b.wagon <= t.cars && b.lift == Lift::Aboard && b.yard == Some(yard)));
        // No two on one place.
        for (i, a) in t.boxes.iter().enumerate() {
            for b in &t.boxes[i + 1..] {
                assert!(!(a.wagon == b.wagon && (a.slot == b.slot || a.forty || b.forty)), "{a:?} and {b:?} share a slot");
            }
        }
    }
    let mut saw_moves = false;
    for _ in 0..400 {
        tick(&mut w, 1);
        let Some(t) = w.rail.trains.get(id) else { break };
        if t.dwelling_at() == Some(yard) {
            let off = t.boxes.iter().filter(|b| matches!(b.lift, Lift::Off(_))).count();
            let on = t.boxes.iter().filter(|b| matches!(b.lift, Lift::On(_))).count();
            if off + on > 0 {
                saw_moves = true;
                assert_eq!(off, 3, "this yard's imports come off");
                assert_eq!(on, 2, "70 units of exports go on in 3 TEU: a forty and a twenty");
                assert!(t.dwell_total >= rail::freight_dwell(5) - 0.01);
            }
        } else if saw_moves {
            assert!(t.boxes.iter().all(|b| b.lift == Lift::Aboard && b.yard.is_none()), "leaves with the exports aboard: {:?}", t.boxes);
            return;
        }
    }
    panic!("the train never called and left (saw moves: {saw_moves})");
}
