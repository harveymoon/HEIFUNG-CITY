//! [stories] Probe: load a save, print its districts as loaded (the story
//! each old district took), then run on for some days and print, once a
//! game day, each district's policy money and figures and the city's.
//! `base` clears every district's policies (as main ran: no district
//! taxes); `stories` keeps the stories the districts took and gives the
//! named districts (or, with none named, the first ones without a story)
//! the stories listed. Used to measure stories before and after on a copy
//! of a real save. Not part of the game.
//!
//! cargo run --profile fast -p nd_sim --example stories_probe -- <save.ron> <base|stories> [days] [district=story ...]

use nd_minds::RuleBasedMind;
use nd_sim::stories::{Policy, Story};
use nd_sim::world::PolicyChange;
use nd_sim::World;

fn report(w: &World) {
    let stats = w.district_stats();
    let adults = w.citizens.values().filter(|c| c.can_work()).count().max(1);
    let idle = w.citizens.values().filter(|c| c.can_work() && c.work.is_none()).count();
    println!(
        "day {} pop {} treasury {:.0} unemployment {:.1}% rent_stressed {} homeless {} open_biz {} | ledger sum {} untracked {} | district levies yday {:.0} spending yday {:.0}",
        w.clock.day(),
        w.citizens.len(),
        w.economy.treasury,
        idle as f32 / adults as f32 * 100.0,
        w.economy.rent_stressed_households,
        w.citizens.values().filter(|c| c.home.is_none()).count(),
        w.buildings.values().filter_map(|b| b.business.as_ref()).filter(|z| !z.closed).count(),
        w.ledger_sum(),
        w.economy.bank.untracked_moves,
        w.economy.ledger.yesterday_of(nd_sim::economy::Flow::DistrictLevies),
        w.economy.ledger.yesterday_of(nd_sim::economy::Flow::DistrictSpending),
    );
    for d in &w.districts.list {
        let Some(s) = stats.iter().find(|s| s.id == d.id) else { continue };
        let by: Vec<String> = Policy::ALL.iter().filter(|p| d.books.yesterday[p.index()] != 0.0).map(|p| format!("{} {:+.0}", p.key(), d.books.yesterday[p.index()])).collect();
        let stressed = w.households.values().filter(|h| h.rent_stress_days > 0 && h.home.and_then(|b| w.buildings.get(b)).and_then(|b| w.lots.get(b.lot)).is_some_and(|l| w.districts.grid.at(l.center) == d.id)).count();
        println!(
            "   {:<16} {:<20} res {:>5} homes {:>5} jobs {:>5}/{:<5} biz {:>3} sites {:>3} bld {:>4} vacant_lots {:>3} rent {:>5.0} stressed_hh {:>4} | {}",
            d.name,
            d.story_label(),
            s.residents,
            s.homes,
            s.workers,
            s.jobs,
            s.businesses_open,
            s.under_construction,
            s.buildings,
            s.vacant_lots,
            s.rent_avg,
            stressed,
            if by.is_empty() { "-".to_string() } else { by.join(" ") }
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("a save");
    let mode = args.get(2).map(String::as_str).unwrap_or("stories");
    let days: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3);
    let mut w = World::read_save(path).expect("loads");
    w.drain_events();
    println!("loaded: {} districts", w.districts.list.len());
    for d in &w.districts.list {
        println!("   {} -> {} ({}){}", d.name, d.story_label(), d.policies.summary(), if d.is_custom() { format!(", closest {:?}", Story::closest(d)) } else { String::new() });
    }
    let ids: Vec<u16> = w.districts.list.iter().map(|d| d.id).collect();
    if mode == "base" {
        for id in ids {
            let pol: Vec<(Policy, f32)> = Policy::ALL.iter().map(|p| (*p, 0.0)).collect();
            w.set_district_policy(id, PolicyChange { policies: pol, ..Default::default() }).unwrap();
        }
    } else {
        for a in &args[4.min(args.len())..] {
            let (name, story) = a.split_once('=').expect("district=story");
            let id = w.districts.find(name).unwrap_or_else(|| panic!("no district {name}"));
            let s = if story == "none" { None } else { Some(Story::parse(story).unwrap_or_else(|| panic!("no story {story}"))) };
            let keep = w.districts.get(id).unwrap().height_limit;
            w.set_district_policy(id, PolicyChange { story: Some(s), ..Default::default() }).unwrap();
            // The measurement is about the policies: the district keeps its height limit.
            w.set_district_policy(id, PolicyChange { height_limit: Some(keep), ..Default::default() }).unwrap();
        }
    }
    println!("running ({mode}):");
    for d in &w.districts.list {
        println!("   {} -> {} ({})", d.name, d.story_label(), d.policies.summary());
    }
    let mut minds = RuleBasedMind::default();
    for _ in 0..days {
        let t = std::time::Instant::now();
        loop {
            w.tick(&mut minds);
            if w.clock.minute_of_day() == 0 {
                break;
            }
        }
        print!("[{:.0}s] ", t.elapsed().as_secs_f64());
        report(&w);
    }
}
