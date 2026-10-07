//! Regression checks on the demo district for REVIEW2's test gaps and this
//! round's sim changes: async mind answers mid-trip (M4), save/load
//! round trips mid-run (L7), park bench seats, and walking access in the
//! land value.
//!
//! cargo test --release -p nd_sim --test sim_checks -- --nocapture

#[path = "demo_city.rs"]
#[allow(dead_code)]
mod demo;

use nd_minds::RuleBasedMind;
use nd_sim::{Activity, CitizenId, Decision, Intent, LandUse, MindContext, MindProvider, World};

/// The rule-based mind, plus answers that arrive "late" through `poll`,
/// as an LLM's do in the app's `HybridMind`.
struct Late {
    rules: RuleBasedMind,
    queue: Vec<(CitizenId, Decision)>,
}

impl MindProvider for Late {
    fn decide(&mut self, ctx: &MindContext) -> Option<Decision> {
        self.rules.decide(ctx)
    }
    fn poll(&mut self) -> Vec<(CitizenId, Decision)> {
        std::mem::take(&mut self.queue)
    }
    fn name(&self) -> &'static str {
        "late"
    }
}

/// Riders listed on a bus who are not on it, and private cars on the road
/// whose owner is not driving them.
fn ghosts(w: &World) -> (usize, usize) {
    let mut bus = 0;
    let mut car = 0;
    for (cid, c) in w.traffic.cars.iter() {
        if let Some(b) = &c.bus {
            bus += b.passengers.iter().filter(|p| !w.citizens.get(**p).is_some_and(|x| matches!(x.activity, Activity::Riding { bus } if bus == cid))).count();
        }
        if c.bus.is_none() && c.cargo.is_none() && c.owned.is_some() && w.citizens.get(c.citizen).is_some_and(|x| !matches!(x.activity, Activity::Driving { car, .. } if car == cid)) {
            car += 1;
        }
    }
    (bus, car)
}

/// REVIEW2 M4: an answer polled for someone driving or on the bus keeps
/// its thought but does not pull them out of the vehicle.
#[test]
fn async_answers_do_not_pull_people_out_of_moving_vehicles() {
    let mut w = demo::demo_world();
    let mut m = Late { rules: RuleBasedMind::default(), queue: Vec::new() };
    for _ in 0..1440 * 3 {
        w.tick(&mut m);
    }
    let mut injected = 0;
    let mut worst = (0, 0);
    let mut thought_seen = false;
    for i in 0..480u64 {
        let mut targets = Vec::new();
        if i % 5 == 0 {
            targets = w.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::Driving { .. } | Activity::Riding { .. })).map(|(id, _)| id).take(2).collect();
            for &id in &targets {
                m.queue.push((id, Decision { intent: Intent::GoHome, thought: "model says: go home".into(), memory: None }));
                injected += 1;
            }
        }
        w.tick(&mut m);
        thought_seen |= targets.iter().any(|&id| w.citizens.get(id).is_some_and(|c| c.thought == "model says: go home"));
        let g = ghosts(&w);
        worst = (worst.0.max(g.0), worst.1.max(g.1));
    }
    assert!(injected >= 20, "answers injected mid-trip: {injected}");
    assert!(thought_seen, "the thought is still taken");
    assert_eq!(worst, (0, 0), "(bus riders not riding, cars driven by someone not in them)");
}

/// REVIEW2 L7, the parts this sim owns: walkers' pavement state and a land
/// value cycle in progress survive a save. (Vehicles do not yet move
/// identically after a load: traffic's own state is a separate matter, so
/// this does not compare positions.)
#[test]
fn a_save_mid_walk_and_mid_land_value_cycle_keeps_both() {
    let mut a = demo::demo_world();
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

/// [benches] Through a day of the demo, nobody shares a seat.
#[test]
fn park_benches_seat_one_person_per_seat() {
    let mut w = demo::demo_world();
    let mut m = RuleBasedMind::default();
    let mut most = 0;
    for t in 0..1440 * 2 {
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
    let mut w = demo::demo_world();
    let mut m = RuleBasedMind::default();
    for _ in 0..1440 * 3 {
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
