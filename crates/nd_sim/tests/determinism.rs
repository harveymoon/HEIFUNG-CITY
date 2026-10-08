//! [review] Whole-world determinism and save/load continuity on the demo
//! district with the rule-based mind: the same seed runs to the same
//! world byte for byte, and (ignored until traffic and rail state is
//! saved) a loaded game goes on exactly as the running one does.
//!
//! cargo test --profile fast -p nd_sim --test determinism -- --nocapture

mod common;

use nd_minds::RuleBasedMind;
use nd_sim::World;

fn run(w: &mut World, m: &mut RuleBasedMind, ticks: u64) {
    for _ in 0..ticks {
        w.tick(m);
    }
}

/// Where two saves first differ, with a little context, for the message.
/// `plan_version` is left out: loading bumps it on purpose (the app's plan
/// caches start afresh), and nothing in the sim reads it.
fn first_difference(a: &str, b: &str) -> Option<String> {
    let keep = |l: &&str| !l.starts_with("    plan_version:");
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().filter(keep).collect(), b.lines().filter(keep).collect());
    let i = la.iter().zip(&lb).position(|(x, y)| x != y).or_else(|| (la.len() != lb.len()).then(|| la.len().min(lb.len())))?;
    let from = i.saturating_sub(12);
    let ctx = |l: &[&str]| l[from.min(l.len())..(i + 3).min(l.len())].join("\n");
    Some(format!("line {i}:\n--- a\n{}\n--- b\n{}", ctx(&la), ctx(&lb)))
}

fn assert_same(a: &World, b: &World, what: &str) {
    let (ra, rb) = (a.to_ron().unwrap(), b.to_ron().unwrap());
    if let Some(d) = first_difference(&ra, &rb) {
        panic!("{what}: the saves differ at {d}");
    }
}

#[test]
fn the_same_seed_runs_to_the_same_world() {
    let (mut a, mut b) = (common::demo_world(), common::demo_world());
    let (mut ma, mut mb) = (RuleBasedMind::default(), RuleBasedMind::default());
    run(&mut a, &mut ma, 2 * 1440);
    run(&mut b, &mut mb, 2 * 1440);
    assert!(a.citizens.len() > 20, "a town grew: {}", a.citizens.len());
    assert_same(&a, &b, "two runs of the same seed");
}

/// Not yet: the world's own state now survives a save (the buses' leg
/// timings, the concern of the week; the services', society's and air's
/// streams on fix/review-sim-core), but state kept outside `world/` is
/// still rebuilt rather than loaded, and the run drifts from the first
/// tick after a load. Found with this test (2026-10, review phase 2a):
/// - `traffic`: the lane network's live state (`Traffic::net`: queues,
///   signal waits), `Car::blocked_by`, `offlaw`, `dodge`, `Traffic::peds`;
///   cars move differently on the very first tick;
/// - `rail`: trains and their orders come back reordered or not at all
///   (`rail.stats.trains` is 0 straight after a load);
/// - `land_value_work`: an access pass in progress restarts differently;
/// - `road_wear`, which follows the traffic.
/// Run it with `--ignored` once those are saved.
#[test]
#[ignore = "traffic, rail and land-value work in progress are not saved yet (see the doc comment)"]
fn a_saved_city_loads_and_goes_on_exactly_as_the_running_one() {
    // Two identical runs (their minds identical too), one of them saved
    // and loaded mid-afternoon on day 2, with emergencies and society on.
    let (mut a, mut b) = (common::demo_world(), common::demo_world());
    let (mut ma, mut mb) = (RuleBasedMind::default(), RuleBasedMind::default());
    let warm = 1440 + 15 * 60 + 7;
    run(&mut a, &mut ma, warm);
    run(&mut b, &mut mb, warm);
    let mut b = World::from_ron(&b.to_ron().unwrap()).unwrap();
    run(&mut a, &mut ma, 1440);
    run(&mut b, &mut mb, 1440);
    assert_same(&a, &b, "the loaded game and the one left running");
}
