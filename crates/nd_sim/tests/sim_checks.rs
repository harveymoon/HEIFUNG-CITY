//! Regression checks on the demo district for REVIEW2's test gaps and this
//! round's sim changes: save/load round trips mid-run (L7), park bench
//! seats, and walking access in the land value.
//!
//! cargo test --release -p nd_sim --test sim_checks -- --nocapture

mod common;

use nd_minds::RuleBasedMind;
use nd_sim::{Activity, LandUse, World};

// [review] REVIEW2 M4 (late answers mid-trip) is checked once, in
// `demo_city::late_mind_answers_wait_until_the_trip_is_over`; this file
// had a second copy of it.

/// REVIEW2 L7, the parts this sim owns: walkers' pavement state and a land
/// value cycle in progress survive a save. (Vehicles do not yet move
/// identically after a load: traffic's own state is a separate matter, so
/// this does not compare positions.)
#[test]
fn a_save_mid_walk_and_mid_land_value_cycle_keeps_both() {
    let mut a = common::demo_world();
    let mut ma = RuleBasedMind::default();
    for _ in 0..1440 * 3 {
        a.tick(&mut ma);
    }
    // Into the land value cycle (strips run at :20.. of every two hours).
    while a.clock.tick % 120 != 22 {
        a.tick(&mut ma);
    }
    let mut b = World::from_ron(&a.to_ron().unwrap()).unwrap();
    assert!(!a.peds.walkers.is_empty(), "people are out walking");
    assert_eq!(a.peds.walkers.len(), b.peds.walkers.len(), "every walker's state is saved");
    for (id, x) in &a.peds.walkers {
        let y = b.peds.walkers.get(id).expect("the same walkers");
        assert_eq!((x.key, x.crossing, x.waiting, x.bold, x.against), (y.key, y.crossing, y.waiting, y.bold, y.against));
        assert!(x.base.distance(y.base) < 1e-3 && (x.side - y.side).abs() < 1e-4);
    }
    let v = a.land_value.version;
    let mut mb = RuleBasedMind::default();
    for _ in 0..8 {
        a.tick(&mut ma);
        b.tick(&mut mb);
    }
    assert_eq!(a.land_value.version, v + 1, "the cycle finished");
    assert_eq!(b.land_value.version, v + 1, "and finished after the load too, rather than being dropped");
}

/// [benches] Through a day of the demo, nobody shares a seat. [freight] Run
/// into the third day: the demo's parks fill from the morning of day 3
/// (until then only the odd night owl sits down, so a two-day window passed
/// or failed on when one stray sitter came, which rail import timing moves).
#[test]
fn park_benches_seat_one_person_per_seat() {
    let mut w = common::demo_world();
    let mut m = RuleBasedMind::default();
    let mut most = 0;
    for t in 0..1440 * 2 + 720 {
        w.tick(&mut m);
        let mut seats: Vec<(i32, i32)> = w.citizens.values().filter_map(|c| if let Activity::Sitting { at, .. } = c.activity { Some(((at.x * 10.0).round() as i32, (at.y * 10.0).round() as i32)) } else { None }).collect();
        let n = seats.len();
        most = most.max(n);
        seats.sort();
        seats.dedup();
        assert_eq!(seats.len(), n, "tick {t}: two people on one seat");
    }
    println!("most sitting at once: {most}; parks found full {}; bench meetings {}", w.seats.full_total, w.seats.met_total);
    assert!(most >= 2, "people sat in the park: {most}");
}

/// [access] In the demo, homes near the park and the clubs have them a
/// short walk away, and the land value carries it.
#[test]
fn demo_homes_know_how_far_the_park_and_nightlife_are() {
    let mut w = common::demo_world();
    let mut m = RuleBasedMind::default();
    // [pt-growth] Four days, not three: building sites work 08:00-17:00 now,
    // and the demo's first night spot (a 5-floor club on this seed) opens
    // on day 4.
    for _ in 0..1440 * 4 {
        w.tick(&mut m);
    }
    let homes: Vec<_> = w.lots.values().filter(|l| l.usable() && l.land_use == LandUse::Residential).map(|l| l.center).collect();
    let access: Vec<_> = homes.iter().filter_map(|&p| w.land_value.access_at(p)).collect();
    assert!(access.len() * 10 >= homes.len() * 9, "nearly every home lot is on the walk network: {} of {}", access.len(), homes.len());
    let parks: Vec<f32> = access.iter().filter_map(|a| a.park_min).collect();
    let nights: Vec<f32> = access.iter().filter_map(|a| a.nightlife_min).collect();
    assert_eq!(parks.len(), access.len(), "every home can walk to the park");
    assert!(!nights.is_empty(), "and to a night out");
    let worst = parks.iter().cloned().fold(0.0, f32::max);
    assert!(worst < 15.0, "a small district: at most {worst} min to the park");
    let mean = access.iter().map(|a| a.value).sum::<f32>() / access.len() as f32;
    println!("homes {}: park {:.1}..{:.1} min, value mean {mean:+.3}", access.len(), parks.iter().cloned().fold(f32::MAX, f32::min), worst);
}
