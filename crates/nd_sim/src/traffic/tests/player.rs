//! [r5-drive] The player's car as the traffic sees it.

use super::*;

/// A straight two-way street and one car heading east along it.
fn street() -> (RoadNetwork, Traffic, CarId, SegmentId) {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let sid = roads.nearest_street(Vec2::ZERO).unwrap().0;
    let lane = lane_of(&t, sid, Vec2::X, 0);
    let len = t.net.lanes[lane as usize].length;
    let car = t.spawn_car(CitizenId::default(), None, vec![lane], 20.0, len - 5.0, 0);
    (roads, t, car, sid)
}

/// The player's car standing in the eastbound lane at `x`, `sideways`
/// metres right of its centre line.
fn parked_at(t: &Traffic, sid: SegmentId, x: f32, sideways: f32) -> PlayerCar {
    let lane = &t.net.lanes[lane_of(t, sid, Vec2::X, 0) as usize];
    let (s, _) = lane.project(Vec2::new(x, 0.0));
    let (q, d) = lane.point_at(s);
    let right = Vec2::new(-d.y, d.x);
    PlayerCar { pos: q + right * sideways, heading: d.y.atan2(d.x), v: 0.0, accel: 0.0, segments: vec![sid], nodes: vec![] }
}

fn runs_to_the_end(t: &mut Traffic, car: CarId) -> bool {
    let mut arrived = false;
    for _ in 0..120 {
        arrived |= t.step(1.0, 4).contains(&car);
        if arrived {
            break;
        }
    }
    arrived
}

#[test]
fn traffic_queues_behind_the_player_and_goes_on_when_it_leaves() {
    let (_, mut t, car, sid) = street();
    t.player = Some(parked_at(&t, sid, 0.0, 0.0));
    let mut held = false;
    for _ in 0..60 {
        t.step(1.0, 4);
        held |= t.player_held.contains(&car);
    }
    let c = &t.cars[car];
    assert!(c.v < 0.2, "stopped behind the player: {} m/s", c.v);
    assert!(held, "counted as held by the player");
    // Its nose (half a car ahead of its position, as it is drawn) short
    // of the player's tail, and not far back.
    let gap = (t.player.as_ref().unwrap().pos - c.pos).length() - PLAYER_HALF_LEN - CAR_LENGTH * 0.5;
    assert!(gap > 0.5 && gap < 8.0, "gap {gap}");
    // The player drives off: the car goes on and arrives.
    t.player = None;
    assert!(runs_to_the_end(&mut t, car), "drove on once the player had gone");
}

#[test]
fn a_player_pulled_onto_the_shoulder_is_passed() {
    let (_, mut t, car, sid) = street();
    // Far enough right to be out of the lane: parked at the kerb.
    t.player = Some(parked_at(&t, sid, 0.0, PLAYER_LATERAL + 0.2));
    assert!(runs_to_the_end(&mut t, car));
    assert!(t.player_held.is_empty());
}

#[test]
fn the_other_way_and_another_level_do_not_care() {
    let (_, mut t, car, sid) = street();
    // In the westbound lane: the eastbound car goes by.
    let mut p = parked_at(&t, sid, 0.0, 0.0);
    p.pos.y = -p.pos.y;
    t.player = Some(p);
    assert!(runs_to_the_end(&mut t, car), "the westbound player does not hold the eastbound lane");
    // On another level (a viaduct over the street): not on its pieces.
    let (_, mut t, car, sid) = street();
    let mut p = parked_at(&t, sid, 0.0, 0.0);
    p.segments.clear();
    t.player = Some(p);
    assert!(runs_to_the_end(&mut t, car), "a player on a level above does not hold the street");
}

#[test]
fn a_moving_player_is_followed_not_rammed() {
    let (_, mut t, car, sid) = street();
    // The player 40 m ahead driving east at 8 m/s.
    let mut p = parked_at(&t, sid, -230.0, 0.0);
    p.v = 8.0;
    let dir = p.dir();
    for _ in 0..30 {
        t.player = Some(p.clone());
        t.step(1.0, 4);
        p.pos += dir * 8.0;
        let c = &t.cars[car];
        let gap = (p.pos - c.pos).dot(dir) - PLAYER_HALF_LEN - CAR_LENGTH * 0.5;
        assert!(gap > 0.0, "never into the player's back: {gap}");
    }
    assert!(t.cars[car].v > 4.0 && t.cars[car].v < 13.5, "follows at about the player's speed: {}", t.cars[car].v);
}

#[test]
fn the_prediction_follows_speed_and_braking() {
    let p = PlayerCar { pos: Vec2::ZERO, heading: 0.0, v: 10.0, accel: 0.0, ..Default::default() };
    assert!((p.predict(2.0).x - 20.0).abs() < 1e-3);
    // Braking at 5 m/s² from 10 m/s stops in 2 s, 10 m on, and stays there.
    let b = PlayerCar { accel: -5.0, ..p.clone() };
    assert!((b.predict(2.0).x - 10.0).abs() < 0.2, "{:?}", b.predict(2.0));
    assert!((b.predict(5.0).x - 10.0).abs() < 0.2, "never reverses: {:?}", b.predict(5.0));
    assert_eq!(b.speed_at(3.0), 0.0);
    // Heading +y in sim axes.
    let n = PlayerCar { heading: std::f32::consts::FRAC_PI_2, ..p };
    assert!((n.predict(1.0) - Vec2::new(0.0, 10.0)).length() < 1e-3);
}

/// Never saved: traffic read back from a save has no player in it.
#[test]
fn the_player_is_not_saved() {
    let (_, mut t, _, sid) = street();
    t.player = Some(parked_at(&t, sid, 0.0, 0.0));
    let text = ron::to_string(&t).expect("serialise");
    assert!(!text.contains("player"), "{}", &text[..text.len().min(300)]);
    let back: Traffic = ron::from_str(&text).expect("deserialise");
    assert!(back.player.is_none());
}

/// No player: every car moves exactly as it does in a run that never had
/// one.
#[test]
fn without_a_player_nothing_changes() {
    let (_, mut a, _, _) = street();
    let (_, mut b, _, sid) = street();
    b.player = Some(parked_at(&b, sid, 250.0, 0.0));
    b.player = None;
    for _ in 0..40 {
        a.step(1.0, 4);
        b.step(1.0, 4);
    }
    let pa: Vec<(f32, f32)> = a.cars.values().map(|c| (c.pos.x, c.v)).collect();
    let pb: Vec<(f32, f32)> = b.cars.values().map(|c| (c.pos.x, c.v)).collect();
    assert_eq!(pa, pb);
}
