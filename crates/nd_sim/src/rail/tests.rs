use super::*;

/// A straight trunk along `TRUNK_Y`, as the world lays it.
fn trunk() -> RoadNetwork {
    let mut net = RoadNetwork::default();
    net.add_polyline_kind(&[Vec2::new(-2000.0, TRUNK_Y), Vec2::new(2000.0, TRUNK_Y)], 0, 0, crate::roads::RoadKind::Rail);
    net
}

#[test]
fn a_branch_snapped_at_both_ends_leaves_and_rejoins_along_the_trunk() {
    let net = trunk();
    // Drawn as a square loop off the trunk and back onto it.
    let drawn = [Vec2::new(-300.0, TRUNK_Y + 3.0), Vec2::new(-300.0, TRUNK_Y + 300.0), Vec2::new(300.0, TRUNK_Y + 300.0), Vec2::new(300.0, TRUNK_Y - 2.0)];
    let built = smooth_track(&drawn, &net);
    assert!(built.len() > drawn.len() * 4, "sampled into a dense curve: {} points", built.len());
    assert!(built.iter().all(|p| p.is_finite()));
    // Both ends sit on the trunk.
    assert!((built[0].y - TRUNK_Y).abs() < 0.01 && (built.last().unwrap().y - TRUNK_Y).abs() < 0.01);
    // And run along it for the switch lead before turning away.
    let (start_dir, end_dir) = (built[1] - built[0], built[built.len() - 1] - built[built.len() - 2]);
    assert!(start_dir.normalize().dot(Vec2::X).abs() > 0.95, "leaves the trunk tangentially: {start_dir:?}");
    assert!(end_dir.normalize().dot(Vec2::X).abs() > 0.95, "arrives tangentially: {end_dir:?}");
    // No corner tighter than the spline can take.
    assert!(sharpest_turn(&built) < 0.6, "sharpest turn {:.2}", sharpest_turn(&built));
    // Lines that touch no track are only eased, not snapped.
    let free = smooth_track(&[Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)], &net);
    assert!(free[0] == Vec2::ZERO && *free.last().unwrap() == Vec2::new(100.0, 100.0));
    assert!(sharpest_turn(&free) < std::f32::consts::FRAC_PI_2 * 0.5);
    assert_eq!(smooth_track(&[Vec2::ZERO], &net), vec![Vec2::ZERO]);
}

#[test]
fn sharpest_turn_measures_the_worst_corner() {
    assert_eq!(sharpest_turn(&[Vec2::ZERO, Vec2::X]), 0.0);
    let right_angle = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(20.0, 10.0)];
    assert!((sharpest_turn(&right_angle) - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
    let gentle = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 1.0)];
    assert!(sharpest_turn(&gentle) < 0.11);
    let reversal = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::ZERO];
    assert!((sharpest_turn(&reversal) - std::f32::consts::PI).abs() < 1e-4);
}

fn train(poly: Vec<Vec2>, s: f32, branch: Option<(f32, f32)>) -> Train {
    Train { kind: TrainKind::Passenger, cars: 2, nodes: Vec::new(), poly, s, prev_s: s, speed: 0.0, stops: Vec::new(), next_stop: 0, dwell: 0.0, passengers: Vec::new(), cargo: Vec::new(), color: 0, held: false, branch, exports: 0.0, unloaded: 0.0, v: 0.0, dwell_total: 0.0, boxes: Vec::new() }
}

/// [freight] Wagons fill oldest first up to capacity; what does not
/// fit waits, smaller loads behind may go, other yards' orders stay,
/// and an empty train always takes the first however big.
#[test]
fn wagons_fill_to_capacity_oldest_first() {
    let mut ids: SlotMap<BuildingId, ()> = SlotMap::with_key();
    let (y1, y2, buyer) = (ids.insert(()), ids.insert(()), ids.insert(()));
    let order = |yard, amount, since| RailOrder { cargo: Cargo { from: yard, to: buyer, resource: crate::market::Resource::Parts, amount, price: 1.0 }, since };
    let mut q = vec![order(y1, 120.0, 5), order(y1, 150.0, 1), order(y2, 30.0, 0), order(y1, 40.0, 7), order(y1, 10.0, 9)];
    let aboard = fill_wagons(&mut q, &[y1], 200.0);
    let got: Vec<f32> = aboard.iter().map(|c| c.amount).collect();
    assert_eq!(got, vec![150.0, 40.0, 10.0], "oldest first; the 120 does not fit and smaller ones go");
    assert!(got.iter().sum::<f32>() <= 200.0);
    assert_eq!(q.len(), 2, "the 120 and the other yard's order wait");
    assert!(q.iter().any(|o| o.cargo.from == y2) && q.iter().any(|o| o.cargo.amount == 120.0));
    let mut big = vec![order(y1, 400.0, 0)];
    assert_eq!(fill_wagons(&mut big, &[y1], 200.0).len(), 1, "an empty train takes an order bigger than itself");
    assert_eq!(wagons_for(0.0), MIN_WAGONS);
    assert_eq!(wagons_for(260.0), 6);
    assert_eq!(wagons_for(1e6), MAX_WAGONS);
    assert!(MAX_WAGONS as f32 * WAGON_CAPACITY >= 400.0, "the largest order fits a full-length train");
}

#[test]
fn blocks_hold_a_train_behind_another_on_the_same_track() {
    let line = vec![Vec2::ZERO, Vec2::new(1000.0, 0.0)];
    let mut rail = Rail::default();
    let behind = rail.trains.insert(train(line.clone(), 200.0, None));
    let ahead = rail.trains.insert(train(line.clone(), 200.0 + BLOCK * 0.5, None));
    assert!(rail.blocked(behind), "the train ahead has its tail in the block");
    assert!(!rail.blocked(ahead), "nothing ahead of the leader");
    // Far enough ahead to be clear.
    rail.trains[ahead].s = 200.0 + BLOCK + rail.trains[ahead].length() + 1.0;
    assert!(!rail.blocked(behind));
    // A train on a different track beside this one does not count.
    let other = vec![Vec2::new(0.0, 10.0), Vec2::new(1000.0, 10.0)];
    let beside = rail.trains.insert(train(other, 230.0, None));
    assert!(!rail.blocked(behind));
    assert!(!rail.blocked(beside));
    assert!(!rail.blocked(TrainId::default()), "an unknown train is never blocked");
}

#[test]
fn calls_ahead_time_the_run_and_skip_what_is_done_or_gone() {
    use slotmap::SlotMap;
    let mut ids: SlotMap<BuildingId, ()> = SlotMap::with_key();
    let (a, b, c) = (ids.insert(()), ids.insert(()), ids.insert(()));
    let mut t = train(vec![Vec2::ZERO, Vec2::new(1000.0, 0.0)], 100.0, None);
    t.speed = 10.0;
    t.stops = vec![(a, 100.0), (b, 400.0), (c, 700.0)];
    // Running towards A (1 s per tick: minutes are seconds here).
    t.s = 50.0;
    let calls = t.calls_ahead(30.0, 1.0, |_| true);
    assert_eq!(calls.iter().map(|c| c.stop).collect::<Vec<_>>(), vec![a, b, c]);
    assert!((calls[0].minutes - 5.0).abs() < 1e-3, "50 m at 10 m/s");
    assert!((calls[1].minutes - (5.0 + 30.0 + 30.0)).abs() < 1e-3, "then a stand and 300 m");
    assert!((calls[2].minutes - (65.0 + 30.0 + 30.0)).abs() < 1e-3);
    assert!((t.minutes_to_end(30.0, 1.0, |_| true) - (125.0 + 30.0 + 30.0)).abs() < 1e-3, "and 300 m on to the end");
    // Four traffic seconds a minute make a quarter of the minutes.
    assert!((t.calls_ahead(30.0, 4.0, |_| true)[0].minutes - 1.25).abs() < 1e-3);
    // Standing at A: A is done, B comes after the rest of the stand.
    t.s = 100.0;
    t.dwell = 12.0;
    assert_eq!(t.dwelling_at(), Some(a));
    let calls = t.calls_ahead(30.0, 1.0, |_| true);
    assert_eq!(calls.iter().map(|c| c.stop).collect::<Vec<_>>(), vec![b, c]);
    assert!((calls[0].minutes - (12.0 + 30.0)).abs() < 1e-3);
    // A stop pulled down is run past: no stand there.
    let calls = t.calls_ahead(30.0, 1.0, |s| s != b);
    assert_eq!(calls.iter().map(|c| c.stop).collect::<Vec<_>>(), vec![c]);
    assert!((calls[0].minutes - (12.0 + 60.0)).abs() < 1e-3);
    // Past every call: nothing ahead, the run out of town remains.
    t.dwell = 0.0;
    t.next_stop = 3;
    t.s = 900.0;
    assert!(t.calls_ahead(30.0, 1.0, |_| true).is_empty());
    assert_eq!(t.calls_made().len(), 3);
    assert!((t.minutes_to_end(30.0, 1.0, |_| true) - 10.0).abs() < 1e-3);
}

#[test]
fn the_route_splits_at_the_head_into_run_and_still_to_run() {
    let poly = vec![Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)];
    let t = train(poly.clone(), 150.0, None);
    let ahead = t.route_ahead(150.0);
    let behind = t.route_behind(150.0);
    assert_eq!(ahead, vec![Vec2::new(100.0, 50.0), Vec2::new(100.0, 100.0)]);
    assert_eq!(behind, vec![Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(100.0, 50.0)]);
    assert!((path_length(&ahead) + path_length(&behind) - 200.0).abs() < 1e-3);
    // Out of range clamps; reversed bounds are put in order; nothing in, nothing out.
    assert_eq!(sub_path(&poly, -10.0, 1e6).len(), 3);
    assert_eq!(sub_path(&poly, 120.0, 20.0), vec![Vec2::new(20.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 20.0)]);
    assert!(sub_path(&[], 0.0, 1.0).is_empty());
}

#[test]
fn state_says_dwelling_held_running_or_leaving() {
    let line = vec![Vec2::ZERO, Vec2::new(2000.0, 0.0)];
    let mut rail = Rail::default();
    let stop = BuildingId::default();
    let mut t = train(line.clone(), 200.0, Some((600.0, 900.0)));
    t.stops = vec![(stop, 1500.0)];
    let me = rail.trains.insert(t);
    assert_eq!(rail.state(me), Some(TrainState::Running));
    // Held with a train in the block: the signal, naming that train.
    let ahead = rail.trains.insert(train(line.clone(), 200.0 + BLOCK * 0.5, None));
    rail.trains[me].held = true;
    assert_eq!(rail.blocked_by(me), Some(ahead));
    assert_eq!(rail.state(me), Some(TrainState::HeldBlock { by: Some(ahead) }));
    // Held at the switch with the block clear: waiting for the branch.
    rail.trains.remove(ahead);
    rail.trains[me].s = 600.0;
    assert_eq!(rail.state(me), Some(TrainState::HeldBranch));
    // Standing at its stop.
    rail.trains[me].held = false;
    rail.trains[me].s = 1500.0;
    rail.trains[me].dwell = 10.0;
    assert_eq!(rail.state(me), Some(TrainState::Dwelling { at: stop, left: 10.0 }));
    // Done calling: leaving town.
    rail.trains[me].dwell = 0.0;
    rail.trains[me].next_stop = 1;
    assert_eq!(rail.state(me), Some(TrainState::Leaving));
    assert_eq!(rail.state(TrainId::default()), None);
}

#[test]
fn on_branch_covers_the_whole_train_not_just_its_head() {
    let line = vec![Vec2::ZERO, Vec2::new(1000.0, 0.0)];
    let mut t = train(line, 0.0, Some((300.0, 600.0)));
    let len = t.length();
    t.s = 100.0;
    assert!(!t.on_branch(), "not yet there");
    t.s = 350.0;
    assert!(t.on_branch(), "head on the branch");
    t.s = 600.0 + len * 0.5;
    assert!(t.on_branch(), "head past the branch, tail still on it");
    t.s = 600.0 + len + 1.0;
    assert!(!t.on_branch(), "clear of it");
    assert!(!train(vec![Vec2::ZERO, Vec2::X], 0.5, None).on_branch(), "no branch on the route");
}
