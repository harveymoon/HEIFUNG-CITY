//! [r15-money] Probe: load a save, run on for some days and print, once a
//! game day, the money crossing the town edge by flow (in from outside, out
//! to outside, per ledger kind) and the town's health: treasury,
//! population, businesses open / broke / closed, households in arrears and
//! the mean purse. Used to compare main and a branch on a copy of a real
//! save. Not part of the game.
//!
//! cargo run --profile fast -p nd_sim --example money_probe -- <save.ron|demo> [days]  (SEED=n for the demo)

use nd_minds::RuleBasedMind;
use nd_sim::bank::{Class, Kind};
use nd_sim::World;
use std::collections::BTreeMap;

fn health(w: &World, label: &str, secs: f64) {
    let e = &w.economy;
    let n = w.households.len().max(1);
    let hh: f64 = w.households.values().map(|h| h.money as f64).sum();
    let arrears = w.households.values().filter(|h| h.money < 0.0).count();
    let open: Vec<_> = w.buildings.values().filter_map(|b| b.business.as_ref()).filter(|z| !z.closed).collect();
    let biz: f64 = open.iter().map(|z| z.money as f64).sum();
    let broke = open.iter().filter(|z| z.money < -300.0).count();
    let shut = w.buildings.values().filter_map(|b| b.business.as_ref()).filter(|z| z.closed).count();
    let adults = w.citizens.values().filter(|c| c.can_work()).count().max(1);
    let idle = w.citizens.values().filter(|c| c.can_work() && c.work.is_none()).count();
    let hungry = w.citizens.values().filter(|c| c.hungry_days > 0).count();
    println!(
        "{label}: day {} pop {} households {} treasury {:.0} mean_purse {:.0} arrears {} biz_open {} broke {} shut {} closed_total {} biz_money {:.0} unemployment {:.1}% hungry {} emigrated {} secs {:.0}",
        w.clock.day(),
        w.citizens.len(),
        w.households.len(),
        e.treasury,
        hh / n as f64,
        arrears,
        open.len(),
        broke,
        shut,
        e.market.closed_total,
        biz,
        idle as f32 / adults as f32 * 100.0,
        hungry,
        e.emigrated_total,
        secs
    );
}

/// Who earns what: everyone's daily income by where it is earned.
fn earnings(w: &World) {
    use nd_sim::{BusinessKind, LandUse};
    let mut by: BTreeMap<&str, (usize, f64)> = BTreeMap::new();
    for c in w.citizens.values() {
        if c.income <= 0.0 {
            continue;
        }
        let what = match c.work.and_then(|b| w.buildings.get(b)) {
            None if c.can_work() => "gigs (no job)",
            None => "pensions and other",
            Some(b) if b.construction.is_some() => "building sites",
            Some(b) if b.service.is_some() || b.land_use == LandUse::Civic => "city services (civic)",
            Some(b) => match &b.business {
                Some(z) if z.closed => "closed business",
                Some(z) if z.kind == BusinessKind::Office => "offices",
                Some(z) if z.kind == BusinessKind::CargoHub => "cargo hubs",
                Some(_) if b.city_parking => "city car parks",
                Some(_) => "business staff",
                None => "other workplace",
            },
        };
        let e = by.entry(what).or_default();
        e.0 += 1;
        e.1 += c.income as f64;
    }
    let s: Vec<String> = by.iter().map(|(k, (n, d))| format!("{k} {n}/{d:.0}")).collect();
    println!("   earnings (people/$ a day): {}", s.join(", "));
    let mut flows: Vec<(f32, &str)> = nd_sim::economy::Flow::ALL.iter().map(|f| (w.economy.ledger.yesterday_of(*f), f.label())).filter(|(v, _)| v.abs() >= 1.0).collect();
    flows.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
    let net: f32 = flows.iter().map(|f| f.0).sum();
    let s: Vec<String> = flows.iter().map(|(v, l)| format!("{l} {v:+.0}")).collect();
    println!("   treasury yesterday net {net:+.0}: {}", s.join(", "));
    // Businesses by land use: open, broke, losing money on their last day.
    let mut lu: BTreeMap<String, (usize, usize, usize, f64)> = BTreeMap::new();
    for b in w.buildings.values().filter(|b| b.operating()) {
        let Some(z) = b.business.as_ref().filter(|z| !z.closed) else { continue };
        let e = lu.entry(format!("{:?}", b.land_use)).or_default();
        e.0 += 1;
        if z.money < -300.0 {
            e.1 += 1;
        }
        if let Some(d) = z.books.last() {
            if d.revenue < d.spend {
                e.2 += 1;
            }
            e.3 += (d.revenue - d.spend) as f64;
        }
    }
    let s: Vec<String> = lu.iter().map(|(k, (n, b, l, p))| format!("{k} {n} open/{b} broke/{l} losing/profit {p:.0}")).collect();
    println!("   businesses: {}", s.join(", "));
    let towers: f64 = w.skyline.developers.iter().map(|d| d.capital).sum();
    let corps: Vec<String> = w.corps.corps.iter().map(|c| format!("{} cash {:.0} debt {:.0} bought {} rev {:.0} held {}", c.brand.name, c.cash, c.debt, c.bought_total, c.revenue_yesterday, w.estate.owners.get(c.owner).map_or(0, |o| o.properties.len()))).collect();
    println!("   towers capital {towers:.0} ({} developers); corps: {}", w.skyline.developers.len(), corps.join("; "));
}

/// Yesterday's flows across the town edge, by kind: in from outside, out
/// to outside; and the in-town total moved (which nets to zero by the door).
fn edge(w: &World) {
    let b = &w.economy.bank;
    let Some(day) = b.yesterday() else { return };
    let mut by: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    let (mut tin, mut tout, mut inside) = (0.0, 0.0, 0.0);
    for c in &day.cells {
        if matches!(c.kind, Kind::Opening) {
            continue;
        }
        let from_out = c.from == Class::Outside;
        let to_out = c.to == Class::Outside;
        if from_out == to_out {
            inside += c.dollars();
            continue;
        }
        let who = if from_out { c.to } else { c.from };
        let e = by.entry(format!("{} ({})", c.kind.label(), who.label())).or_default();
        if from_out {
            e.0 += c.dollars();
            tin += c.dollars();
        } else {
            e.1 += c.dollars();
            tout += c.dollars();
        }
    }
    println!("   edge: in {tin:.0} out {tout:.0} net {:+.0}; moved inside town {inside:.0}; ledger sum {} untracked {:.2}", tin - tout, w.ledger_sum(), nd_sim::bank::dollars(b.untracked_units));
    for (k, (i, o)) in by {
        if i.abs() < 0.5 && o.abs() < 0.5 {
            continue;
        }
        println!("     {k:<52} in {i:>12.0} out {o:>12.0}");
    }
    if std::env::var_os("MONEY_INSIDE").is_some() {
        let mut ins: BTreeMap<String, f64> = BTreeMap::new();
        for c in &day.cells {
            if c.from != Class::Outside && c.to != Class::Outside {
                *ins.entry(format!("{:<26} {:>14} -> {:<14}", c.kind.label(), c.from.label(), c.to.label())).or_default() += c.dollars();
            }
        }
        for (k, v) in ins {
            println!("     inside {k} {v:>12.0}");
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).expect("save path");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
    let mut w = if src == "demo" {
        let mut w = World::new(nd_sim::WorldConfig { seed: std::env::var("SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(1), ..nd_sim::WorldConfig::default() });
        w.drain_events();
        nd_sim::scenarios::demo_district(&mut w);
        w
    } else {
        World::read_save(src).expect("load")
    };
    let mut minds = RuleBasedMind::default();
    health(&w, "start", 0.0);
    earnings(&w);
    for _ in 0..days {
        let t0 = std::time::Instant::now();
        loop {
            w.tick(&mut minds);
            w.drain_events();
            if w.clock.minute_of_day() == 0 {
                break;
            }
        }
        health(&w, "day", t0.elapsed().as_secs_f64());
        earnings(&w);
        edge(&w);
    }
}
