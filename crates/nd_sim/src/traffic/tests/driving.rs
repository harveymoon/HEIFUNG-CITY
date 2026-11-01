//! [driving] Driver profiles, drink, rage, crashes and stunts.

use super::*;

/// [inspector] The slow–normal–reckless reading of a driver.
#[test]
fn boldness_runs_from_slow_to_reckless() {
    let ordinary = DriverProfile::for_driver(40, 0.5, 0.3, 0.0, 0.5, 0.0).boldness();
    let careful_old = DriverProfile::for_driver(85, 0.95, 0.05, 0.0, 0.5, 0.0).boldness();
    let rebel = DriverProfile::for_driver(21, 0.05, 0.95, 0.0, 0.5, 0.0).boldness();
    let drunk_rebel = DriverProfile::for_driver(21, 0.05, 0.95, 1.0, 0.5, 0.0).boldness();
    assert!(ordinary.abs() < 0.25, "ordinary {ordinary}");
    assert!(careful_old < -0.25, "careful pensioner {careful_old}");
    assert!(rebel > 0.45, "rebel {rebel}");
    assert!(drunk_rebel >= rebel);
    assert!(DriverProfile::professional(0.0).boldness().abs() < 0.25);
    assert_eq!(DriverProfile::default().boldness(), 0.0, "unassigned reads as normal");
    assert!(DriverProfile::for_driver(40, 0.5, 0.3, 0.0, 0.5, 0.0).angry(1.0).boldness() > ordinary, "anger shows");
}

#[test]
fn driver_profiles_change_speed_and_following_distance() {
    let old = DriverProfile::for_driver(82, 0.9, 0.2, 0.0, 0.5, 0.1);
    let ordinary = DriverProfile::for_driver(40, 0.5, 0.5, 0.0, 0.5, 0.2);
    let reckless = DriverProfile::for_driver(20, 0.05, 0.95, 0.0, 0.5, 0.3);
    let (v_old, gap_old) = cruise(old);
    let (v_ord, gap_ord) = cruise(ordinary);
    let (v_rec, gap_rec) = cruise(reckless);
    assert!(v_old < 0.85 * ROAD_SPEED, "the old drive slowly: {v_old:.1}");
    assert!(v_rec > 1.12 * ROAD_SPEED, "reckless youth over the limit: {v_rec:.1}");
    assert!(v_old < v_ord && v_ord < v_rec, "{v_old:.1} {v_ord:.1} {v_rec:.1}");
    assert!(gap_rec < gap_ord && gap_ord < gap_old, "closer behind the leader the more reckless: {gap_rec:.1} {gap_ord:.1} {gap_old:.1}");
    assert!(reckless.compliance < ordinary.compliance && ordinary.compliance < 1.0);
    assert!(old.reaction > ordinary.reaction && reckless.aggression > ordinary.aggression);
    // The unassigned default drives the plain model.
    assert!(!DriverProfile::default().assigned && DriverProfile::default().weave_amplitude() == 0.0);
}

#[test]
fn drunk_drivers_swerve_more_and_crash_more_often() {
    let (mut drunk_crashes, mut sober_crashes) = (0, 0);
    let (mut drunk_weave, mut sober_weave) = (0.0f32, 0.0f32);
    for seed in 1..=4 {
        let (c, _, wv, _) = drink_trial(0.85, seed);
        drunk_crashes += c;
        drunk_weave = drunk_weave.max(wv);
        let (c, _, wv, _) = drink_trial(0.0, seed);
        sober_crashes += c;
        sober_weave = sober_weave.max(wv);
    }
    assert!(drunk_weave > 0.9 && sober_weave < 0.25, "drunks drift over the lines: {drunk_weave:.2} vs {sober_weave:.2}");
    assert!(drunk_crashes >= 3 * sober_crashes.max(1), "drunk {drunk_crashes} crashes vs sober {sober_crashes}");
    // Seeded: the same night twice.
    assert_eq!(drink_trial(0.85, 3), drink_trial(0.85, 3));
}

/// The inspector's "why is it standing there": a queue behind a wreck
/// names the car in front of each, and the chain leads back to the
/// wreck at its head.
#[test]
fn a_queue_explains_itself_back_to_the_wreck_at_its_head() {
    let roads = street_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    let head = send(&mut t, &roads, Vec2::new(-300.0, 0.5 * w), Vec2::new(150.0, 0.5 * w));
    for _ in 0..3 {
        t.step(1.0, 4);
    }
    t.crash(vec![head], CrashCause::OffRoad, 25.0);
    t.crashes.clear();
    let mut queue = Vec::new();
    for _ in 0..3 {
        queue.push(send(&mut t, &roads, Vec2::new(-340.0, 0.5 * w), Vec2::new(150.0, 0.5 * w)));
        for _ in 0..12 {
            t.step(1.0, 4);
        }
    }
    for _ in 0..20 {
        t.step(1.0, 4);
    }
    assert_eq!(t.stop_reason(head), StopReason::Crashed { wrecked: true });
    let last = *queue.last().unwrap();
    assert_eq!(t.stop_reason(last), StopReason::Queued { behind: queue[1] });
    assert!(t.stop_reason(last).text(|_| "X".into()).contains("queued behind X"));
    let chain = t.jam_chain(last);
    assert!(!chain.cycle);
    assert_eq!(chain.head(), Some(head), "the chain leads to the wreck: {:?}", chain.cars);
    assert_eq!(chain.cars, vec![queue[2], queue[1], queue[0], head]);
    assert!(t.stop_reason(head).is_good(), "a wreck is a reason");
}

#[test]
fn road_rage_starts_late_for_the_patient_and_is_deterministic() {
    // On time, or a little late: no anger.
    assert_eq!(road_rage(10.0, 10.0, 0.5, 0.5), 0.0);
    assert_eq!(road_rage(18.0, 10.0, 0.5, 0.5), 0.0, "late, but shrugged off");
    // A short hop doubling is not worth getting angry about.
    assert_eq!(road_rage(4.0, 2.0, 0.0, 1.0), 0.0);
    // Far over: angry, more so for the impatient and hot-headed.
    let (impatient, patient) = (road_rage(30.0, 10.0, 0.1, 0.8), road_rage(30.0, 10.0, 0.9, 0.2));
    assert!(impatient > 0.5, "{impatient}");
    assert!(patient < impatient, "{patient} vs {impatient}");
    assert_eq!(road_rage(32.0, 10.0, 0.9, 0.2), road_rage(32.0, 10.0, 0.9, 0.2), "deterministic");
    // It builds with time and is capped; the even-tempered cap lower.
    let mut last = 0.0;
    for m in 0..200 {
        let a = road_rage(m as f32, 10.0, 0.3, 0.6);
        assert!(a >= last && a <= 1.0);
        last = a;
    }
    assert!(road_rage(1000.0, 10.0, 0.5, 0.0) < road_rage(1000.0, 10.0, 0.5, 1.0));
    // Anger makes a driver tailgate, floor it, push in and misjudge.
    let calm = DriverProfile::for_driver(35, 0.6, 0.4, 0.0, 0.7, 0.3);
    assert_eq!(calm.angry(0.0), calm);
    let mad = calm.angry(1.0);
    assert!(mad.headway < calm.headway && mad.accel > calm.accel && mad.aggression > calm.aggression);
    assert!(mad.skill < calm.skill && mad.compliance < calm.compliance && mad.speed > calm.speed);
}

#[test]
fn a_crash_stops_both_cars_and_a_minor_one_clears() {
    let roads = street_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    let a = send(&mut t, &roads, Vec2::new(-300.0, 0.5 * w), Vec2::new(-100.0, 0.5 * w));
    let b = send(&mut t, &roads, Vec2::new(-280.0, 0.5 * w), Vec2::new(-100.0, 0.5 * w));
    for _ in 0..5 {
        t.step(1.0, 4);
    }
    t.crash(vec![a, b], CrashCause::RearEnd, 3.0);
    let r = t.crashes.pop().expect("filed");
    assert!(r.severity < BAD_CRASH, "a slow bump is minor");
    let follower = send(&mut t, &roads, Vec2::new(-340.0, 0.5 * w), Vec2::new(-100.0, 0.5 * w));
    for _ in 0..20 {
        t.step(1.0, 4);
    }
    assert!(t.cars[a].crashed() && t.cars[a].v == 0.0 && t.cars[b].v == 0.0);
    assert_eq!(t.cars[follower].wait, Wait::Leader, "traffic backs up behind");
    for _ in 0..200 {
        t.step(1.0, 4);
    }
    assert!(!t.cars.get(a).is_some_and(|c| c.crashed()), "a minor crash drives on after a while");
    // A bad one is written off and stays.
    let c = send(&mut t, &roads, Vec2::new(-300.0, -0.5 * w + w), Vec2::new(-100.0, 0.5 * w));
    t.crash(vec![c], CrashCause::OffRoad, 25.0);
    assert!(t.crashes.pop().unwrap().severity >= BAD_CRASH);
    for _ in 0..400 {
        t.step(1.0, 4);
    }
    assert!(t.cars[c].drive.crash.is_some_and(|s| s.wrecked));
}

#[test]
fn a_blocked_junction_holds_traffic_and_stunts_leave_the_lanes() {
    let roads = street_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    let node = t.net.intersections.iter().find(|i| roads.node(i.node).pos.length() < 1.0).unwrap().node;
    t.set_blocked(node, true);
    let car = send(&mut t, &roads, Vec2::new(-200.0, 0.5 * w), Vec2::new(200.0, 0.5 * w));
    for _ in 0..60 {
        t.step(2.0, 4);
    }
    let c = &t.cars[car];
    assert_eq!(c.idx, 0, "never entered the junction");
    assert_eq!(c.wait, Wait::Blocked);
    // Into a donut: off the lane, spinning on the spot.
    t.begin_stunt(car, StuntKind::Donut { centre: Vec2::new(0.0, 0.0), radius: 2.4, dir: 1.0 });
    assert!(t.net.lanes.iter().all(|l| !l.cars.contains(&car)));
    let mut headings = Vec::new();
    for _ in 0..30 {
        t.step(2.0, 4);
        headings.push(t.cars[car].heading);
    }
    let c = &t.cars[car];
    assert!(c.pos.length() < 3.0, "spinning in the junction: {:?}", c.pos);
    assert!(headings.windows(2).any(|h| (h[1] - h[0]).abs() > 0.5), "it turns");
    t.set_blocked(node, false);
    let (route, s0, s1) = t.net.find_route(roads.nearest_street(Vec2::new(100.0, 0.0)).unwrap().0, Vec2::new(5.0, 0.5 * w), roads.nearest_street(Vec2::new(200.0, 0.0)).unwrap().0, Vec2::new(200.0, 0.5 * w)).unwrap();
    t.end_stunt(car, route, s0, s1);
    let mut arrived = false;
    for _ in 0..100 {
        if t.step(2.0, 4).contains(&car) {
            arrived = true;
            break;
        }
    }
    assert!(arrived, "back on the lanes and away");
}

/// [r5-fixes] The day 5-6 crash on fresh generated cities ("invalid
/// SlotMap key" at a crossing's siren look-up): a takeover car in its
/// show hit a walker and was pulled in to the kerb; the kerb then pulled
/// it out into its old lane's list while it carried on with the show, the
/// end of the show listed it on its new lane too, and the old entry
/// outlived the car. A car in a stunt now stays off every lane list until
/// `end_stunt`, and is listed once, on the lane it drives.
#[test]
fn a_car_in_a_stunt_pulled_in_stays_off_the_lanes_and_leaves_no_stale_entry() {
    let roads = cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let west = roads.nearest_street(Vec2::new(-100.0, 0.0)).unwrap().0;
    let north = roads.nearest_street(Vec2::new(0.0, 100.0)).unwrap().0;
    let (a, b) = (lane_of(&t, west, Vec2::X, 0), lane_of(&t, north, Vec2::Y, 0));
    let car = t.spawn_car(CitizenId::default(), None, vec![a], 50.0, 150.0, 0);
    let listed = |t: &Traffic| -> Vec<usize> { t.net.lanes.iter().enumerate().filter(|(_, l)| l.cars.contains(&car)).map(|(i, _)| i).collect() };
    assert_eq!(listed(&t), vec![a as usize]);
    t.begin_stunt(car, StuntKind::Ring { at: Vec2::new(-20.0, 5.0), face: 0.0 });
    assert!(listed(&t).is_empty(), "off the lanes for the show");
    // A walker hit: the world pulls the car in.
    t.pull_in(car, 150.0);
    assert!(!t.cars[car].entering, "not waiting at the kerb: it is in the show");
    for _ in 0..20 {
        t.step(1.0, 4);
    }
    assert!(listed(&t).is_empty(), "the kerb never pulls a car in a show into a lane: {:?}", listed(&t));
    // The show ends on a new route: listed there only.
    t.end_stunt(car, vec![b], 20.0, 150.0);
    assert!(listed(&t) == vec![b as usize] || (t.cars[car].entering && listed(&t).is_empty()), "{:?}", listed(&t));
    for _ in 0..5 {
        t.step(1.0, 4);
    }
    assert!(listed(&t).len() <= 1, "listed once: {:?}", listed(&t));
    // Removed: no lane names it, and the crossings' look-ups hold.
    t.remove_car(car);
    assert!(listed(&t).is_empty());
    assert!(!t.siren_near(a, 100.0));
    // Even an id left behind by some other slip is only skipped.
    t.net.lanes[a as usize].cars.push(car);
    assert!(!t.siren_near(a, 100.0) && !t.siren_near(b, 10.0));
    assert!(t.time_to_crossing(a, 100.0, 3.0).is_infinite());
    t.step(1.0, 4);
    assert!(listed(&t).is_empty(), "the next step drops it");
}
