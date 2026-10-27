//! [companies] Probe: load a save (or the demo district), run on for some
//! days and print who owns the town once a game day: the biggest owners by
//! properties (and what each is), owners by kind, how many local companies
//! there are, the fleets (cabs, tow trucks, garbage trucks per company) and
//! the liveries on the road, with unemployment and the ledger's sum. Used to
//! compare the companies work before and after on a copy of a real save.
//! Not part of the game.
//!
//! cargo run --profile fast -p nd_sim --example company_probe -- <save.ron|demo> [days]

use nd_minds::RuleBasedMind;
use nd_sim::World;
use std::collections::BTreeMap;

/// The owner kind's name without its fields ("Firm", "Developer").
fn kind_name(k: &nd_sim::estate::OwnerKind) -> String {
    let s = format!("{k:?}");
    s.split([' ', '{', '(']).next().unwrap_or("").to_string()
}

fn report(w: &World, label: &str, secs: f64) {
    let adults = w.citizens.values().filter(|c| c.can_work()).count().max(1);
    let idle = w.citizens.values().filter(|c| c.can_work() && c.work.is_none()).count();
    println!("{label}: day {} pop {} buildings {} unemployment {:.1}% ledger_sum {} secs {:.1}", w.clock.day(), w.citizens.len(), w.buildings.len(), idle as f32 / adults as f32 * 100.0, w.ledger_sum(), secs);
    let mut by_kind: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for o in w.estate.owners.values() {
        let e = by_kind.entry(kind_name(&o.kind)).or_default();
        e.0 += 1;
        e.1 += o.properties.len();
    }
    println!("   owners by kind (owners/properties): {}", by_kind.iter().map(|(k, (n, p))| format!("{k} {n}/{p}")).collect::<Vec<_>>().join(", "));
    let local = w.estate.owners.values().filter(|o| matches!(kind_name(&o.kind).as_str(), "Firm" | "Person" | "Company" | "Builder" | "MegaCorp")).count();
    println!("   local companies (firms, founders, property companies, builders, corps): {local}");
    let mut top: Vec<_> = w.estate.owners.values().collect();
    top.sort_by(|a, b| b.properties.len().cmp(&a.properties.len()).then(a.name.cmp(&b.name)));
    println!("   top owners: {}", top.iter().take(12).map(|o| format!("{} [{}] {}", o.name, kind_name(&o.kind), o.properties.len())).collect::<Vec<_>>().join("; "));
    // Fleets: cabs, tow trucks and garbage trucks per company.
    let mut fleets: Vec<(String, String, usize)> = Vec::new();
    for (id, b) in w.buildings.iter() {
        let Some(z) = &b.business else { continue };
        let n = match z.kind {
            nd_sim::BusinessKind::TaxiCompany => w.taxis.fleet_size(id),
            nd_sim::BusinessKind::TowYard => w.tow_trucks(id),
            nd_sim::BusinessKind::TrashCompany => w.society.trash.trucks.values().filter(|t| t.hauler == nd_sim::trash::Hauler::Company(id)).count(),
            _ => continue,
        };
        fleets.push((b.name.clone(), z.kind.label().to_string(), n));
    }
    fleets.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    println!("   fleets ({}): {}", fleets.len(), fleets.iter().map(|(n, k, c)| format!("{n} ({k}) {c}")).collect::<Vec<_>>().join("; "));
    let mut liv: BTreeMap<String, usize> = BTreeMap::new();
    for c in w.traffic.cars.values() {
        let l = format!("{:?}", c.model.livery);
        if l != "None" {
            *liv.entry(l).or_default() += 1;
        }
    }
    println!("   liveries on the road ({} kinds): {}", liv.len(), liv.iter().map(|(k, n)| format!("{k} {n}")).collect::<Vec<_>>().join(", "));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).map(String::as_str).unwrap_or("demo");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let mut w = if src == "demo" {
        let mut w = World::new(nd_sim::WorldConfig { seed: 1, ..nd_sim::WorldConfig::default() });
        w.drain_events();
        nd_sim::scenarios::demo_district(&mut w);
        w
    } else {
        World::read_save(src).expect("load")
    };
    let mut minds = RuleBasedMind::default();
    report(&w, "start", 0.0);
    for _ in 0..days {
        let t0 = std::time::Instant::now();
        loop {
            w.tick(&mut minds);
            w.drain_events();
            if w.clock.minute_of_day() == 0 {
                break;
            }
        }
        report(&w, "day", t0.elapsed().as_secs_f64());
    }
}
