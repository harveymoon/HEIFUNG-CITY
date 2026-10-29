//! [r4-timetable] Probe: load a save, run to the second midnight after it
//! (so one whole day is in the books) and report transit by mode: boardings,
//! the wait at the stop (from reaching it to boarding), riders who missed
//! the last bus or train, the home-to-work commute, and what the whole day
//! cost. Env `TT` sets timetables first (branch r4-timetable only):
//! `TT=bus:commuter,metro:24h,rail:commuter,ferry:commuter`.
//!
//! cargo run --profile fast -p nd_sim --example timetable_probe -- saves/autosave.ron

use nd_sim::citizens::{Activity, CitizenId};
use nd_sim::transit::TransitLeg;
use nd_sim::World;
use std::collections::BTreeMap;

fn kind(l: &TransitLeg) -> &'static str {
    match l {
        TransitLeg::Bus { .. } => "bus",
        TransitLeg::Subway { .. } => "metro",
        TransitLeg::Rail { .. } => "rail",
        TransitLeg::Taxi => "taxi",
        TransitLeg::Air { .. } => "air",
    }
}

fn setup(w: &mut World, spec: &str) {
    use nd_sim::timetable::{Kind, Preset};
    for item in spec.split(',') {
        let Some((kind, preset)) = item.split_once(':') else { continue };
        let preset = Preset::from_key(preset).expect("preset");
        for line in w.timetabled_lines() {
            if line.kind().key() == kind || (kind == "all" && line.kind() != Kind::Rail) {
                let t = w.set_line_preset(line, preset).unwrap();
                println!("{} {}: {} {:?}", kind, w.line_name(line).unwrap_or_default(), t.hours_text(), t.values);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let mut w = World::read_save(&src).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    println!("loaded day {} {:02}:{:02}, {} people", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.citizens.len());
    if let Ok(spec) = std::env::var("TT") {
        setup(&mut w, &spec);
    }
    let start_day = w.clock.day();
    println!("fleet wanted/depot room {:?}", w.fleet_balance());
    // Whole days: from the day after the save's (`DAYS` of them, 1 by
    // default), read at the midnight after each.
    let days: u64 = std::env::var("DAYS").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    let end = (w.clock.tick / 1440 + 1 + days) * 1440 + 1;
    let full_from = (w.clock.tick / 1440 + 1) * 1440;
    let mut waiting: BTreeMap<CitizenId, (&'static str, u64)> = BTreeMap::new();
    let mut boards: BTreeMap<&'static str, (u32, u64)> = BTreeMap::new();
    let mut missed: BTreeMap<(&'static str, u32), u32> = BTreeMap::new();
    let mut gave_up: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut hourly: BTreeMap<(&'static str, u32), (u32, u64)> = BTreeMap::new();
    let mut left_home: BTreeMap<CitizenId, u64> = BTreeMap::new();
    let mut inside: BTreeMap<CitizenId, Option<nd_sim::buildings::BuildingId>> = w.citizens.iter().map(|(id, c)| (id, c.inside)).collect();
    let mut commutes: Vec<f32> = Vec::new();
    let mut costs = String::new();
    let mut late = String::new();
    while w.clock.tick < end {
        w.tick(&mut minds);
        w.drain_events();
        let tick = w.clock.tick;
        let full = tick > full_from;
        let mut now: BTreeMap<CitizenId, (&'static str, u64)> = BTreeMap::new();
        for (id, c) in w.citizens.iter() {
            if let (Activity::Waiting { .. }, Some(tr)) = (&c.activity, c.transit) {
                now.insert(id, (kind(&tr.leg), tr.since));
            }
        }
        if full {
            for (id, (k, since)) in &waiting {
                if now.get(id).is_some_and(|n| n.1 == *since) {
                    continue;
                }
                let Some(c) = w.citizens.get(*id) else { continue };
                match &c.activity {
                    Activity::Riding { .. } | Activity::OnMetro { .. } | Activity::OnTrain { .. } | Activity::Flying { .. } => {
                        let e = boards.entry(k).or_default();
                        e.0 += 1;
                        e.1 += tick.saturating_sub((*since).max(1));
                        let e = hourly.entry((k, w.clock.hour() as u32)).or_default();
                        e.0 += 1;
                        e.1 += tick.saturating_sub((*since).max(1));
                    }
                    _ if c.thought.starts_with("Missed the last") => *missed.entry((k, w.clock.hour() as u32)).or_default() += 1,
                    _ => *gave_up.entry(k).or_default() += 1,
                }
            }
            for (id, c) in w.citizens.iter() {
                let before = inside.get(&id).copied().flatten();
                if before == c.inside {
                    continue;
                }
                if before.is_some() && before == c.home && c.inside.is_none() {
                    left_home.insert(id, tick);
                }
                if c.inside.is_some() && c.inside == c.work {
                    if let Some(t0) = left_home.remove(&id) {
                        commutes.push((tick - t0) as f32);
                    }
                }
                inside.insert(id, c.inside);
            }
        } else {
            for (id, c) in w.citizens.iter() {
                inside.insert(id, c.inside);
            }
        }
        waiting = now;
        if tick % 1440 == 1439 && full {
            late = format!("passenger trains {}, ferry riders {}", w.rail.stats.passenger_trains_today, w.ports.stats.ferry_riders_today);
        }
        if tick % 1440 == 0 && tick > full_from {
            let r = &w.rail.stats;
            costs = format!(
                "day {} books: buses+depots {:.0} (lines {:.0}), metro {:.0}, rail stations {:.0}, metro riders {}, bus riders {}, money {:.0}",
                w.clock.day() - 1,
                w.economy.transit.upkeep_today,
                w.transit.routes.values().map(|r| r.upkeep_yesterday).sum::<f32>(),
                w.subway.stats.upkeep_today,
                r.upkeep_today,
                w.subway.stats.riders_yesterday,
                w.transit.routes.values().map(|r| r.riders_yesterday).sum::<u32>(),
                w.economy.treasury,
            );
        }
    }
    println!("== the whole of days {}..={} (from the save on day {start_day}), per day:", start_day + 1, start_day as u64 + days);
    for (k, (n, wait)) in &boards {
        println!("boardings {k:6} {:6}  mean wait {:.1} min", *n as u64 / days, *wait as f32 / (*n).max(1) as f32);
    }
    for k in ["bus", "metro"] {
        let row: Vec<String> = (0..24).map(|h| hourly.get(&(k, h)).map_or("-".to_string(), |(n, wt)| format!("{h}:{}@{:.0}", *n as u64 / days, *wt as f32 / (*n).max(1) as f32))).collect();
        println!("hourly {k}: {}", row.join(" "));
    }
    let total_missed: u32 = missed.values().sum();
    println!("missed the last bus/train: {} a day", total_missed as u64 / days);
    for ((k, h), n) in &missed {
        println!("   {k:6} {h:02}h {n}");
    }
    println!("left the wait another way: {gave_up:?}");
    let n = commutes.len().max(1) as f32;
    let mut s = commutes.clone();
    s.sort_by(|a, b| a.total_cmp(b));
    println!("home->work commutes {} mean {:.1} min median {:.0} p90 {:.0}", commutes.len(), commutes.iter().sum::<f32>() / n, s.get(s.len() / 2).copied().unwrap_or(0.0), s.get(s.len() * 9 / 10).copied().unwrap_or(0.0));
    println!("{costs}");
    println!("{late}");
    // A save to look at the timelines in the game (`SAVE=path`), run on to
    // `SAVE_AT` minutes into the next day (default 17:00).
    if let Ok(path) = std::env::var("SAVE") {
        let at: u64 = std::env::var("SAVE_AT").ok().and_then(|s| s.parse().ok()).unwrap_or(17 * 60);
        while w.clock.minute_of_day() as u64 != at {
            w.tick(&mut minds);
            w.drain_events();
        }
        let f = std::fs::File::create(&path).expect("save file");
        w.write_ron_gz(std::io::BufWriter::new(f)).expect("save written");
        println!("saved {path} at day {} {:02}:{:02}", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60);
    }
    // [r4-timetable] Missed counted by the lines themselves, and runs.
    let tl: Vec<(String, (u32, Option<f32>, u32, u32))> = w.timetabled_lines().into_iter().filter_map(|l| Some((w.line_name(l)?, w.line_timeline(l)?.totals(true)))).collect();
    for (name, (b, wait, missed, runs)) in tl {
        println!("  line {name:20} boarded {b:5} wait {:5.1} missed {missed:4} runs {runs}", wait.unwrap_or(0.0));
    }
    for l in w.timetabled_lines() {
        let (Some((t, _)), Some(tl)) = (w.line_schedule(l), w.line_timeline(l)) else { continue };
        let parts: Vec<String> = nd_sim::timetable::Part::ALL.iter().zip(tl.by_part(&t, true)).filter(|(_, (b, _))| *b > 0).map(|(p, (b, wt))| format!("{} {b} @{:.0}", p.key(), wt.unwrap_or(0.0))).collect();
        println!("  by part {:20} {}", w.line_name(l).unwrap_or_default(), parts.join(" | "));
    }
}
