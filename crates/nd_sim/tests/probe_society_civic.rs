//! Regression (review 2, H2 and M5): civic structures (police, hospital,
//! power plant, ...) neither decay to derelict nor burn down. A fire guts
//! one (out of service), the city repairs it for money over a few days,
//! and the building is never removed. (The review's probe asserted the
//! defects; it now asserts the fix.)
//!
//! cargo test --release -p nd_sim --test probe_society_civic -- --nocapture --test-threads=1

use nd_minds::RuleBasedMind;
use nd_sim::services::{Outcome, BURNED_DOWN};
use nd_sim::{BuildingId, LandUse, ServiceKind, Tier, World};

mod common;

use common::place;

fn to_midnight(w: &mut World, minds: &mut RuleBasedMind) {
    loop {
        w.tick(minds);
        if w.clock.minute_of_day() == 0 {
            break;
        }
    }
}

#[test]
fn civic_structures_never_decay_or_burn_down_and_fire_damage_is_repaired() {
    let mut w = common::demo_world();
    w.economy.treasury = 1e7;
    let mut minds = RuleBasedMind::default();
    let police = place(&mut w, ServiceKind::Police, Tier::Low, &[LandUse::Civic, LandUse::Commercial]);
    let health = place(&mut w, ServiceKind::Health, Tier::Mid, &[LandUse::Civic, LandUse::Commercial]);
    let plant = w.buildings.iter().find(|(_, b)| b.plant.is_some()).map(|(id, _)| id);
    let stations = [("police (low)", police), ("hospital (mid)", health)];
    let start: Vec<f32> = stations.iter().map(|(_, b)| w.buildings[*b].condition).collect();
    let days = 4;
    for day in 1..=days {
        to_midnight(&mut w, &mut minds);
        let line: Vec<String> = stations
            .iter()
            .map(|(n, b)| {
                let x = &w.buildings[*b];
                format!("{n}: condition {:.3} workers {}/{} derelict {} crewed {}", x.condition, x.workers.len(), x.jobs, x.is_derelict(), w.crewed_units(*b))
            })
            .collect();
        println!("day {day}: pop {} | {}", w.citizens.len(), line.join(" | "));
    }
    for (k, (n, b)) in stations.iter().enumerate() {
        let now = w.buildings[*b].condition;
        // A random fire may hit a station in these days (the later part
        // tests fire damage and its repair); decay is what must not happen.
        let burned = w.services.incidents.iter().any(|i| i.building == *b && i.kind == nd_sim::services::IncidentKind::Fire);
        assert!(now >= start[k] || burned, "{n} decayed: {} -> {now}", start[k]);
    }
    // Run down (as by an old save's decay): mended at the next midnight.
    w.buildings[police].condition = 0.305;
    to_midnight(&mut w, &mut minds);
    for _ in 0..12 {
        w.tick(&mut minds);
    }
    let x = &w.buildings[police];
    println!("police after one more midnight: condition {:.3} derelict {} crewed_units {}", x.condition, x.is_derelict(), w.crewed_units(police));
    assert!(!x.is_derelict() && x.condition > 0.305, "repaired, not left to rot: {}", x.condition);

    // Fire: no fire station in town; set the hospital (and the coal plant) alight.
    let fires: Vec<(&str, BuildingId)> = [("hospital", Some(health)), ("power plant", plant)].into_iter().filter_map(|(n, b)| b.map(|b| (n, b))).collect();
    let mut incidents = Vec::new();
    for (_, b) in &fires {
        let r = w.start_fire(*b);
        println!("start_fire -> {r:?}");
        incidents.push(r.expect("a civic building can catch fire"));
    }
    let mut done = false;
    for t in 0..3000 {
        w.tick(&mut minds);
        for (n, b) in &fires {
            assert!(w.buildings.contains_key(*b), "t+{t}: the {n} burned down and was removed");
            assert!(w.buildings[*b].condition >= BURNED_DOWN - 1e-6);
        }
        if incidents.iter().all(|i| !w.services.incident(*i).unwrap().active()) {
            println!("t+{t}: fires over: {:?}", incidents.iter().map(|i| w.services.incident(*i).unwrap().outcome).collect::<Vec<_>>());
            done = true;
            break;
        }
    }
    assert!(done, "the fires end");
    for i in &incidents {
        let o = w.services.incident(*i).unwrap().outcome;
        assert!(matches!(o, Some(Outcome::Gutted | Outcome::Extinguished)), "{o:?}");
    }
    // Gutted ones are out of service, then repaired over the next days,
    // and the bill is in the ledger (booked to the day that closed).
    let mut repair_bill = 0.0;
    for _ in 0..4 {
        to_midnight(&mut w, &mut minds);
        repair_bill -= w.economy.ledger.items_yesterday.iter().filter(|e| matches!(&e.item, nd_sim::economy::Item::Named(n) if n.starts_with("repairs to "))).map(|e| e.amount).sum::<f32>();
    }
    println!("repairs paid: {repair_bill:.0}");
    for (n, b) in &fires {
        let x = &w.buildings[*b];
        println!("{n}: condition {:.3} derelict {}", x.condition, x.is_derelict());
        assert!(!x.is_derelict(), "the {n} was repaired: {}", x.condition);
    }
    assert!(repair_bill > 0.0, "repairs were paid for");
}
