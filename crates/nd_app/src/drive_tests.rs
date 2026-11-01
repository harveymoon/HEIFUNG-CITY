//! [r5-drive] Drive mode's pure parts: the keys and the pad, the car's
//! handling, bumps, the surface it keeps to, the lane assist and the
//! compass.

use super::*;
use std::f32::consts::{FRAC_PI_2, PI};

fn keys(down: &[KeyCode]) -> (Controls, Look) {
    drive_keys(|k| down.contains(&k))
}

#[test]
fn the_keys_drive_steer_and_look() {
    assert_eq!(keys(&[]).0, Controls::default());
    assert_eq!(keys(&[KeyCode::KeyW]).0.throttle, 1.0);
    assert_eq!(keys(&[KeyCode::ArrowUp]).0.throttle, 1.0);
    assert_eq!(keys(&[KeyCode::KeyS]).0.brake, 1.0);
    assert_eq!(keys(&[KeyCode::ArrowDown]).0.brake, 1.0);
    assert_eq!(keys(&[KeyCode::KeyA]).0.steer, -1.0);
    assert_eq!(keys(&[KeyCode::ArrowRight]).0.steer, 1.0);
    // Both ways at once cancel.
    assert_eq!(keys(&[KeyCode::KeyA, KeyCode::KeyD]).0.steer, 0.0);
    assert!(keys(&[KeyCode::Space]).0.handbrake);
    assert!(keys(&[KeyCode::KeyH]).0.horn);
    // The camera: Q / E turn, R / F and Home / End tilt, . , zoom, C behind.
    assert_eq!(keys(&[KeyCode::KeyQ]).1.turn, 1.0);
    assert_eq!(keys(&[KeyCode::KeyE]).1.turn, -1.0);
    assert_eq!(keys(&[KeyCode::KeyR]).1.tilt, 1.0);
    assert_eq!(keys(&[KeyCode::End]).1.tilt, -1.0);
    assert_eq!(keys(&[KeyCode::Period]).1.zoom, 1.0);
    assert_eq!(keys(&[KeyCode::Comma]).1.zoom, -1.0);
    assert!(keys(&[KeyCode::KeyC]).1.behind);
    // Driving keys do not move the camera, nor camera keys the car.
    assert_eq!(keys(&[KeyCode::KeyW, KeyCode::KeyD]).1, Look::default());
    assert_eq!(keys(&[KeyCode::KeyQ, KeyCode::KeyR, KeyCode::KeyC]).0, Controls::default());
}

#[test]
fn the_pad_drives_too() {
    let m = |pan: Vec2, zoom: f32, yaw: f32, tilt: f32| NavMove { pan, zoom, yaw, tilt };
    let (c, l) = pad_drive(m(Vec2::Y, 0.0, 0.0, 0.0));
    assert_eq!((c.throttle, c.brake, l), (1.0, 0.0, Look::default()));
    let (c, _) = pad_drive(m(-Vec2::Y, 0.0, 0.0, 0.0));
    assert_eq!((c.throttle, c.brake), (0.0, 1.0));
    assert_eq!(pad_drive(m(Vec2::X, 0.0, 0.0, 0.0)).0.steer, 1.0);
    // A tap scaled up for a frame stays a full press, no more.
    assert_eq!(pad_drive(m(Vec2::new(-15.0, 15.0), 0.0, 0.0, 0.0)).0, Controls { throttle: 1.0, steer: -1.0, ..Default::default() });
    let (_, l) = pad_drive(m(Vec2::ZERO, 1.0, 1.0, -1.0));
    assert_eq!(l, Look { turn: 1.0, tilt: -1.0, zoom: 1.0, behind: false });
    // Keys and pad together: the stronger of each.
    let both = Controls { throttle: 0.5, ..Default::default() }.with(Controls { throttle: 1.0, steer: -0.3, horn: true, ..Default::default() });
    assert_eq!(both, Controls { throttle: 1.0, steer: -0.3, horn: true, ..Default::default() });
}

/// Flat open road everywhere.
fn open(_: Vec2, f: f32) -> Result<DriveGround, DriveBlock> {
    Ok(DriveGround::Road { segment: nd_sim::roads::SegmentId(0), height: f })
}

fn run(car: &mut DriveCar, c: Controls, secs: f32) {
    for _ in 0..(secs * 60.0) as usize {
        car.step(&c, 1.0 / 60.0, &open, &[]);
    }
}

#[test]
fn it_pulls_away_tops_out_brakes_and_reverses() {
    let mut car = DriveCar::new(Vec2::ZERO, 0.0, 0.0);
    run(&mut car, Controls { throttle: 1.0, ..Default::default() }, 3.0);
    // About town speed in a few seconds.
    assert!((8.0..14.0).contains(&car.v), "{} m/s after 3 s", car.v);
    run(&mut car, Controls { throttle: 1.0, ..Default::default() }, 60.0);
    assert!(car.v <= TOP_SPEED + 0.01 && car.v > TOP_SPEED * 0.88, "top {}", car.v);
    assert!(car.pos.y.abs() < 1e-3 && car.pos.x > 500.0, "straight on: {:?}", car.pos);
    // From the top speed the brake stops it within about 5 s, and then it
    // reverses (slowly).
    let mut t = 0.0;
    while car.v > 0.0 && t < 10.0 {
        car.step(&Controls { brake: 1.0, ..Default::default() }, 1.0 / 60.0, &open, &[]);
        t += 1.0 / 60.0;
    }
    assert!(t < 5.0, "stopped in {t} s");
    run(&mut car, Controls { brake: 1.0, ..Default::default() }, 6.0);
    assert!(car.v < -3.0 && car.v >= -REVERSE_TOP - 0.01, "reverses: {}", car.v);
    // The accelerator brakes a reversing car before it goes forward.
    run(&mut car, Controls { throttle: 1.0, ..Default::default() }, 0.3);
    assert!(car.v > -REVERSE_TOP && car.v < 0.5);
    // Let go: it rolls to a stop and stays.
    run(&mut car, Controls::default(), 30.0);
    assert_eq!(car.v, 0.0);
}

#[test]
fn it_steers_right_and_left_and_less_at_speed() {
    // Steering right (+) turns the heading up (towards +y), as the sim's
    // right is (-d.y, d.x).
    let mut car = DriveCar::new(Vec2::ZERO, 0.0, 0.0);
    car.v = 8.0;
    run(&mut car, Controls { throttle: 0.3, steer: 1.0, ..Default::default() }, 1.0);
    assert!(car.heading > 0.2 && car.pos.y > 0.0, "right: {} {:?}", car.heading, car.pos);
    let mut l = DriveCar::new(Vec2::ZERO, 0.0, 0.0);
    l.v = 8.0;
    run(&mut l, Controls { throttle: 0.3, steer: -1.0, ..Default::default() }, 1.0);
    assert!(l.heading < -0.2 && l.pos.y < 0.0);
    // The wheel eases in: a one-frame tap is a nudge, not full lock.
    let mut t = DriveCar::new(Vec2::ZERO, 0.0, 0.0);
    t.step(&Controls { steer: 1.0, ..Default::default() }, 1.0 / 60.0, &open, &[]);
    assert!(t.wheel > 0.0 && t.wheel < 0.2);
    // At 30 m/s a second of full right is a gentle curve (no spin): the
    // sideways pull stays under about 1 g.
    let mut fast = DriveCar::new(Vec2::ZERO, 0.0, 0.0);
    fast.v = 30.0;
    let h0 = fast.heading;
    run(&mut fast, Controls { throttle: 1.0, steer: 1.0, ..Default::default() }, 1.0);
    let rate = (fast.heading - h0).abs();
    assert!(rate > 0.05 && rate * fast.v < 12.0, "yaw {rate} rad over 1 s at {} m/s", fast.v);
    // Standing still, the wheel turns nothing.
    let mut still = DriveCar::new(Vec2::ZERO, 0.0, 0.0);
    run(&mut still, Controls { steer: 1.0, ..Default::default() }, 1.0);
    assert_eq!((still.heading, still.pos), (0.0, Vec2::ZERO));
}

#[test]
fn the_handbrake_slows_and_swings_the_tail() {
    let mut a = DriveCar::new(Vec2::ZERO, 0.0, 0.0);
    a.v = 15.0;
    let mut b = a;
    run(&mut a, Controls { steer: 1.0, ..Default::default() }, 0.8);
    run(&mut b, Controls { steer: 1.0, handbrake: true, ..Default::default() }, 0.8);
    assert!(b.v < a.v - 3.0, "slower: {} vs {}", b.v, a.v);
    assert!(b.heading > a.heading * 0.9, "turns at least as sharply");
}

/// A wall across the road at x = 30 (a building, say); open before it.
fn walled(p: Vec2, f: f32) -> Result<DriveGround, DriveBlock> {
    if p.x > 30.0 { Err(DriveBlock::Building) } else { open(p, f) }
}

#[test]
fn a_wall_bumps_it_back_not_through() {
    let mut car = DriveCar::new(Vec2::ZERO, 0.0, 0.0);
    let mut bumped = false;
    for _ in 0..600 {
        bumped |= car.step(&Controls { throttle: 1.0, ..Default::default() }, 1.0 / 60.0, &walled, &[]);
        assert!(car.pos.x + HALF_LEN <= 30.0 + 0.01, "never into the wall: {:?}", car.pos);
    }
    assert!(bumped && car.bumps >= 1);
    assert_eq!(car.last_bump, Some(Bump::Scenery(DriveBlock::Building)));
    // A glancing hit slides along the wall instead of stopping dead.
    let mut glance = DriveCar::new(Vec2::new(20.0, 0.0), 0.3, 0.0);
    glance.v = 10.0;
    let y0 = glance.pos.y;
    run_on(&mut glance, Controls { throttle: 0.5, ..Default::default() }, 2.0, &walled);
    assert!(glance.pos.y > y0 + 3.0, "slid along: {:?}", glance.pos);
}

fn run_on(car: &mut DriveCar, c: Controls, secs: f32, g: &impl Fn(Vec2, f32) -> Result<DriveGround, DriveBlock>) {
    for _ in 0..(secs * 60.0) as usize {
        car.step(&c, 1.0 / 60.0, g, &[]);
    }
}

#[test]
fn it_bumps_other_cars_and_stops_short_of_them() {
    let parked = Body { pos: Vec2::new(25.0, 0.0), heading: 0.0, half_len: 2.25, half_wid: 0.9 };
    let mut car = DriveCar::new(Vec2::ZERO, 0.0, 0.0);
    let mut bumped = false;
    for _ in 0..400 {
        bumped |= car.step(&Controls { throttle: 1.0, ..Default::default() }, 1.0 / 60.0, &open, &[parked]);
        assert!(!car.body().overlaps(&parked), "never inside it");
    }
    assert!(bumped && car.last_bump == Some(Bump::Car));
    // One lane over: it goes by.
    let beside = Body { pos: Vec2::new(25.0, 3.5), ..parked };
    let mut car = DriveCar::new(Vec2::ZERO, 0.0, 0.0);
    for _ in 0..300 {
        car.step(&Controls { throttle: 1.0, ..Default::default() }, 1.0 / 60.0, &open, &[beside]);
    }
    assert!(car.pos.x > 40.0 && car.bumps == 0);
}

#[test]
fn bodies_overlap_by_their_real_shape() {
    let a = Body { pos: Vec2::ZERO, heading: 0.0, half_len: 2.3, half_wid: 0.95 };
    assert!(a.overlaps(&Body { pos: Vec2::new(4.0, 0.0), ..a }));
    assert!(!a.overlaps(&Body { pos: Vec2::new(4.7, 0.0), ..a }));
    assert!(!a.overlaps(&Body { pos: Vec2::new(0.0, 2.0), ..a }));
    // Turned across: its length now runs sideways.
    assert!(a.overlaps(&Body { pos: Vec2::new(0.0, 3.0), heading: FRAC_PI_2, ..a }));
    assert!(!a.overlaps(&Body { pos: Vec2::new(3.4, 0.0), heading: FRAC_PI_2, ..a }));
}

/// A ramp climbing 1 m per 10 m from x = 0, with a parapet: off its 8 m
/// width is a drop.
fn ramp(p: Vec2, f: f32) -> Result<DriveGround, DriveBlock> {
    let h = (p.x * 0.1).max(0.0);
    if p.y.abs() > 4.0 && h > 1.0 {
        return Err(DriveBlock::Wall);
    }
    if (h - f).abs() > nd_sim::DRIVE_STEP {
        return Err(DriveBlock::Wall);
    }
    Ok(DriveGround::Road { segment: nd_sim::roads::SegmentId(0), height: h })
}

#[test]
fn it_keeps_to_the_road_up_a_ramp_and_not_off_a_deck() {
    let mut car = DriveCar::new(Vec2::new(-20.0, 0.0), 0.0, 0.0);
    run_on(&mut car, Controls { throttle: 0.6, ..Default::default() }, 8.0, &ramp);
    assert!(car.pos.x > 40.0, "{:?}", car.pos);
    assert!((car.floor - car.pos.x * 0.1).abs() < 0.2, "on the ramp's surface: floor {} at x {}", car.floor, car.pos.x);
    // Up on the deck, turned to the side: the parapet stops it.
    let mut side = DriveCar::new(Vec2::new(60.0, 0.0), FRAC_PI_2, 6.0);
    side.v = 6.0;
    run_on(&mut side, Controls { throttle: 0.5, ..Default::default() }, 3.0, &ramp);
    assert!(side.pos.y + HALF_LEN < 4.0 + 0.5 && side.bumps >= 1, "{:?}", side.pos);
    assert!((side.floor - 6.0).abs() < 0.5, "still on the deck: {}", side.floor);
}

#[test]
fn off_the_road_is_slow() {
    let off = |_: Vec2, f: f32| -> Result<DriveGround, DriveBlock> { Ok(DriveGround::Off { height: f }) };
    let mut car = DriveCar::new(Vec2::ZERO, 0.0, 0.0);
    run_on(&mut car, Controls { throttle: 1.0, ..Default::default() }, 20.0, &off);
    assert!(!car.on_road && car.v <= OFF_ROAD_TOP + 0.01, "{}", car.v);
}

#[test]
fn a_car_stuck_in_a_wall_can_drive_out() {
    let mut car = DriveCar::new(Vec2::new(40.0, 0.0), PI, 0.0);
    run_on(&mut car, Controls { throttle: 1.0, ..Default::default() }, 2.0, &walled);
    assert!(car.pos.x < 35.0, "{:?}", car.pos);
}

#[test]
fn the_lane_assist_steers_back_to_the_lane() {
    // On course: nothing.
    assert_eq!(assist_steer(0.0, 0.0, 0.0, 10.0), 0.0);
    // Right of the lane's centre: steer left (-), and the other way.
    assert!(assist_steer(0.0, 0.0, 1.0, 10.0) < 0.0);
    assert!(assist_steer(0.0, 0.0, -1.0, 10.0) > 0.0);
    // Pointing left of the lane: steer right.
    assert!(assist_steer(-0.2, 0.0, 0.0, 10.0) > 0.0);
    // Parked: it leaves the wheel alone.
    assert_eq!(assist_steer(0.0, 0.0, 1.0, 0.5), 0.0);
    // With the assist the car settles onto the lane's line.
    let mut car = DriveCar::new(Vec2::new(0.0, 1.2), 0.1, 0.0);
    car.v = 12.0;
    for _ in 0..600 {
        let s = assist_steer(car.heading, 0.0, car.pos.y, car.v);
        car.step(&Controls { throttle: 0.4, steer: s, ..Default::default() }, 1.0 / 60.0, &open, &[]);
    }
    assert!(car.pos.y.abs() < 0.3 && wrap(car.heading).abs() < 0.05, "{:?} {}", car.pos, car.heading);
}

#[test]
fn the_compass_reads_like_the_map() {
    // North is up the map as the orbit starts: -y in sim axes.
    assert!((bearing(-FRAC_PI_2) - 0.0).abs() < 0.01 || (bearing(-FRAC_PI_2) - 360.0).abs() < 0.01);
    assert!((bearing(0.0) - 90.0).abs() < 0.01);
    assert!((bearing(FRAC_PI_2) - 180.0).abs() < 0.01);
    assert!((bearing(PI) - 270.0).abs() < 0.01);
    assert_eq!(compass_point(0.0), "N");
    assert_eq!(compass_point(44.0), "NE");
    assert_eq!(compass_point(91.0), "E");
    assert_eq!(compass_point(359.0), "N");
    assert_eq!(compass_point(225.0), "SW");
}
