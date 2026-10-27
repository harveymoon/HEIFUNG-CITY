//! [ledger] Probe: load a save (or the demo district), run on for some days
//! and print the money picture once a game day: treasury, household and
//! business cash, broke and closed businesses, rent stress, the unhoused,
//! and the wall time per day (tick cost). Used to compare a change before
//! and after on a copy of a real save. Not part of the game.
//!
//! cargo run --profile fast -p nd_sim --example ledger_probe -- <save.ron|demo> [days]

use nd_minds::RuleBasedMind;
use nd_sim::World;

fn line(w: &World, label: &str, secs: f64) {
    let e = &w.economy;
    let hh: f64 = w.households.values().map(|h| h.money as f64).sum();
    let open: Vec<_> = w.buildings.values().filter_map(|b| b.business.as_ref()).filter(|z| !z.closed).collect();
    let biz: f64 = open.iter().map(|z| z.money as f64).sum();
    let broke = open.iter().filter(|z| z.money < -300.0).count();
    let shut = w.buildings.values().filter_map(|b| b.business.as_ref()).filter(|z| z.closed).count();
    println!(
        "{label}: day {} pop {} hh {} treasury {:.0} hh_money {:.0} biz_money {:.0} open_biz {} broke {} shut {} closed_total {} rent_stressed {} unhoused {} homeless {} buildings {} secs {:.1}",
        w.clock.day(),
        w.citizens.len(),
        w.households.len(),
        e.treasury,
        hh,
        biz,
        open.len(),
        broke,
        shut,
        e.market.closed_total,
        e.rent_stressed_households,
        e.unhoused,
        w.citizens.values().filter(|c| c.home.is_none()).count(),
        w.buildings.len(),
        secs
    );
}

/// [ledger] The books: the balance check, any drift, yesterday's flows.
fn books(w: &World) {
    let b = &w.economy.bank;
    println!("   ledger: sum {} accounts {} untracked {:.2} in {} moves orphans {} drifts {}", w.ledger_sum(), b.accounts(), nd_sim::bank::dollars(b.untracked_units), b.untracked_moves, b.orphans, b.drifts.len());
    for d in b.drifts.iter().rev().take(8) {
        println!("     drift tick {} {} {:+.2} after {:?}", d.tick, d.account, d.dollars, d.last_kind);
    }
    if let Some(day) = b.yesterday() {
        let mut by: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
        for c in &day.cells {
            *by.entry(format!("{:<26} {:>14} -> {:<14}", c.kind.label(), c.from.label(), c.to.label())).or_default() += c.dollars();
        }
        for (k, v) in by {
            println!("     {k} {v:>14.0}");
        }
    }
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
    line(&w, "start", 0.0);
    for _ in 0..days {
        let t0 = std::time::Instant::now();
        loop {
            w.tick(&mut minds);
            w.drain_events();
            if w.clock.minute_of_day() == 0 {
                break;
            }
        }
        line(&w, "day", t0.elapsed().as_secs_f64());
        if std::env::var_os("LEDGER_BOOKS").is_some() {
            books(&w);
        }
    }
    if std::env::var_os("LEDGER_COST").is_some() {
        cost(&w);
    }
}

/// [ledger] What the door costs: moves a day, the time of a move (timed on
/// a copy, households paying each other), and what the books add to a save.
fn cost(w: &World) {
    use nd_sim::bank::{Account, Kind};
    let moves = w.economy.bank.history.back().map_or(0, |d| d.cells.iter().map(|c| c.n as u64).sum::<u64>());
    let mut c = w.clone();
    let hh: Vec<_> = c.households.keys().collect();
    let n = 400_000usize;
    let t0 = std::time::Instant::now();
    for i in 0..n {
        let a = hh[i % hh.len()];
        let b = hh[(i * 7 + 3) % hh.len()];
        c.transfer(Account::Household(a), Account::Household(b), 1.25, Kind::Gifts);
    }
    let per = t0.elapsed().as_secs_f64() / n as f64;
    let t1 = std::time::Instant::now();
    c.reconcile_ledger();
    let rec = t1.elapsed().as_secs_f64();
    let bank = ron::to_string(&w.economy.bank).unwrap();
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gz, bank.as_bytes()).unwrap();
    let gz = gz.finish().unwrap();
    let mut all = Vec::new();
    w.write_ron_gz(&mut all).unwrap();
    println!(
        "ledger cost: {moves} moves yesterday, {:.0} ns a move -> {:.3} s a day; a full reconcile {:.1} ms; books in the save {} bytes ({} gz) of {} gz",
        per * 1e9,
        per * moves as f64,
        rec * 1e3,
        bank.len(),
        gz.len(),
        all.len()
    );
}
