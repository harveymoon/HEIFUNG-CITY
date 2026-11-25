//! [r17-fixes] Parking fines probe (r16 photo F14): load a save and run on,
//! printing once a game day the population, treasury and the city's main
//! incomes (income, property and business tax, parking fines), the day's
//! tickets, clamps and tows, and why each new ticket was written: parked
//! where it may not (overflow, a banned kerb) or over the kerb limit, and
//! where the owner was (at work, at home, out). Not part of the game.
//!
//! cargo run --profile fast -p nd_sim --example fines_probe -- save.ron [days=1]

use nd_minds::RuleBasedMind;
use nd_sim::economy::Flow;
use nd_sim::{CarPlace, ParkKind, World};
use std::collections::BTreeMap;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&a[1]).expect("load");
    let days: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut minds = RuleBasedMind::default();
    println!("loaded: day {} {:02}:{:02} pop {} treasury {:.0} cars {}", w.clock.day(), w.clock.hour(), w.clock.minute(), w.citizens.len(), w.economy.treasury, w.vehicles.cars.len());
    let mut seen: BTreeMap<nd_sim::vehicle::VehicleId, u64> = BTreeMap::new();
    for _ in 0..days {
        let mut why: BTreeMap<String, u32> = BTreeMap::new();
        let mut kerb_samples = (0u64, 0u64, 0u64, 0u64);
        let mut day_stats = None;
        for m in 0..1440 {
            w.tick(&mut minds);
            if m == 1438 {
                day_stats = Some(w.enforcement.stats.clone());
            }
            if m % 60 == 30 {
                for c in w.vehicles.cars.values() {
                    if let CarPlace::Parked(s) = &c.place {
                        if s.kind.on_street() {
                            kerb_samples.0 += 1;
                            if s.kind == ParkKind::Overflow {
                                kerb_samples.1 += 1;
                            } else if w.parked_illegally(s) {
                                kerb_samples.2 += 1;
                            }
                            if s.ticketed > 0 {
                                kerb_samples.3 += 1;
                            }
                        }
                    }
                }
            }
            for (id, c) in w.vehicles.cars.iter() {
                let CarPlace::Parked(s) = &c.place else { continue };
                if s.ticketed == 0 || seen.get(&id) == Some(&s.ticketed) {
                    continue;
                }
                seen.insert(id, s.ticketed);
                if s.ticketed + 1 < w.clock.tick {
                    continue;
                }
                let reason = if s.kind == ParkKind::Overflow {
                    "overflow"
                } else if w.parked_illegally(s) {
                    "banned kerb"
                } else {
                    "over limit"
                };
                let owner = w.citizens.get(c.owner);
                let at = match owner.map(|o| &o.activity) {
                    Some(nd_sim::citizens::Activity::Working) => "owner at work",
                    Some(nd_sim::citizens::Activity::AtHome) => "owner at home",
                    Some(nd_sim::citizens::Activity::Visiting(_)) => "owner visiting",
                    Some(_) => "owner out",
                    None => "no owner",
                };
                let stood = (s.ticketed.saturating_sub(s.since)) / 60;
                *why.entry(format!("{reason:<12} {at:<14}")).or_default() += 1;
                *why.entry(format!("~ parked {:>2} h before ticket", stood.min(12))).or_default() += 1;
            }
        }
        let e = day_stats.expect("a day");
        let l = &w.economy.ledger;
        println!(
            "== day {} pop {} treasury {:.0}: income tax {:.0} property tax {:.0} business tax {:.0} parking fines {:.0} (tickets {} = {:.0}, clamps {} paid {} = {:.0}, tows {})",
            w.clock.day(),
            w.citizens.len(),
            w.economy.treasury,
            l.yesterday_of(Flow::IncomeTax),
            l.yesterday_of(Flow::PropertyTax),
            l.yesterday_of(Flow::BusinessTax),
            l.yesterday_of(Flow::ParkingFines),
            e.tickets_today,
            e.fines_today - e.clamp_fines_today,
            e.clamps_today,
            e.clamps_paid_today,
            e.clamp_fines_today,
            e.tows_today
        );
        let n = kerb_samples.0.max(1) as f64;
        println!("   street-parked cars (mean of hourly samples) {:.0}: overflow {:.0}, banned kerb {:.0}, carrying a ticket {:.0}", kerb_samples.0 as f64 / 24.0, kerb_samples.1 as f64 / 24.0, kerb_samples.2 as f64 / 24.0, kerb_samples.3 as f64 / 24.0);
        let _ = n;
        for (k, v) in &why {
            println!("   tickets: {k} {v}");
        }
    }
}
