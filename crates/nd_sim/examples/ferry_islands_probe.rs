//! [ferry-islands] Headless look at a city's islands: the crossings, the
//! islands found, the stranded areas left, and day by day the ferry
//! figures (residents carried, loads and lorries across, missed last
//! boats, households moving off) with the rush-hour crowds on the piers.
//!
//! `ferry_islands_probe <save.ron> [days]`; `ND_SAVE_OUT=<path>` writes the
//! city after the run. Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::transit::TransitLeg;
use nd_sim::{Activity, World};

pub fn summary(w: &World) {
    let name = |b: nd_sim::BuildingId| w.buildings.get(b).map_or("?".to_string(), |x| x.name.clone());
    let terms: Vec<_> = w.ports.ports.values().filter(|p| p.kind.is_ferry()).collect();
    println!("ferry terminals {}: {}", terms.len(), terms.iter().map(|p| format!("{} ({}, works {}, walk {} road {}, at {:.0},{:.0})", p.building.map_or("-".into(), name), p.kind.label(), p.works(), p.walk_connected, p.road_connected, p.center.x, p.center.y)).collect::<Vec<_>>().join("; "));
    for p in &terms {
        let Some(b) = p.building else { continue };
        let mine: Vec<String> = w.ports.ships.values().filter(|s| s.port == b).map(|s| format!("{:?} to {:?} s {:.0}/{:.0} docked {} dwell {} pass {} riders {} cargo {}", s.kind, s.to.map(name), s.s, nd_sim::geom::path_length(&s.path), s.docked, s.dwell, s.passengers, s.riders.len(), s.cargo.len())).collect();
        let bb = &w.buildings[b];
        println!("    {} derelict {} operating {} site {} workers {}/{} timetable {:?} headway now {:?}", name(b), bb.is_derelict(), bb.operating(), bb.construction.is_some(), bb.workers.len(), bb.jobs, p.timetable.is_some(), p.ferry_headway_at(w.clock.minute_of_day() as u32));
        println!("  {}: sailings {} last {} remote {:?} ships {:?}", name(b), p.sailings, p.last_sailing, p.remote.iter().map(|r| r.name.clone()).collect::<Vec<_>>(), mine);
    }
    let mut q: std::collections::BTreeMap<String, u32> = Default::default();
    for l in &w.ports.ferry_freight {
        *q.entry(format!("{} -> {}", name(l.at), name(l.to))).or_default() += 1;
    }
    println!("  freight queue {:?}; ships {} (big {})", q, w.ports.ships.len(), w.ports.ships.values().filter(|s| s.kind.is_big()).count());
    for l in &w.ports.links {
        println!("  crossing {} <-> {}: {:.0} m{}", name(l.a), name(l.b), l.metres, if l.cars { ", car ferry" } else { "" });
    }
    for i in w.islands() {
        println!("  island '{}' at ({:.0},{:.0}): {} buildings, {} live, {} work; sail {:.0} min, wait now {:?}; car ferry {}; police {} fire {} health {}; loads waiting {}", i.name, i.center.x, i.center.y, i.buildings, i.residents, i.workers, i.sail_minutes, i.wait_minutes.map(|x| x.round()), i.car_ferry, i.police, i.fire, i.health, i.loads_waiting);
    }
    for i in &w.connectivity.islands {
        let homes: Vec<_> = i.buildings.iter().filter_map(|b| w.buildings.get(*b)).filter(|b| b.land_use == nd_sim::zoning::LandUse::Residential).collect();
        println!("  island comp {} lots {} terminals {:?}; homes {} (open {}, free homes {}, tiers {:?}, rents {:?})", i.comp, i.lots.len(), i.terminals.iter().map(|b| name(*b)).collect::<Vec<_>>(), homes.len(), homes.iter().filter(|b| b.operating()).count(), homes.iter().map(|b| b.free_homes() as u32).sum::<u32>(), homes.iter().map(|b| b.tier).collect::<Vec<_>>(), homes.iter().map(|b| b.rent.round()).collect::<Vec<_>>());
    }
    // Who waits on the piers, by where they live and the pier.
    let mut who: std::collections::BTreeMap<String, u32> = Default::default();
    for c in w.citizens.values() {
        let Some(TransitLeg::Ferry { board, alight }) = c.transit.map(|t| t.leg) else { continue };
        if !matches!(c.activity, Activity::Waiting { .. }) {
            continue;
        }
        let home = c.home.map_or("homeless", |h| if w.island_of(h).is_some() { "island" } else if w.connectivity.walk_cut.contains(&w.buildings[h].lot) { "stranded" } else { "main" });
        *who.entry(format!("{home} {}->{} {:?}", name(board), name(alight), c.transit.map(|t| t.purpose))).or_default() += 1;
    }
    for (k, n) in who {
        println!("    waiting: {n} {k}");
    }
    if std::env::var("ND_WHO").is_ok() {
        for c in w.citizens.values().filter(|c| matches!(c.activity, Activity::Waiting { .. }) && matches!(c.transit.map(|t| t.leg), Some(TransitLeg::Ferry { .. })) && c.home.is_some_and(|h| w.island_of(h).is_some())).take(8) {
            let mem: Vec<String> = c.memories.iter().rev().take(5).map(|m| format!("{:02}:{:02} {}", (m.tick % 1440) / 60, m.tick % 60, m.text)).collect();
            println!("      {} work {:?} shift {:?} | {} | {:?}", c.name, c.work.map(name), c.work.and_then(|j| w.buildings.get(j)).map(|b| b.shift_for(c.seed)), c.thought, mem);
        }
    }
    for a in w.stranded_areas() {
        println!("  stranded {:?} comp {}: {} (at {:.0},{:.0}, box {:.0},{:.0}..{:.0},{:.0})", a.net, a.comp, a.describe(w), a.center.x, a.center.y, a.lo.x, a.lo.y, a.hi.x, a.hi.y);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&args[1]).expect("load");
    let days: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    w.refresh_connectivity();
    // ND_PRESET=<timetable preset key>: every ferry terminal on it.
    if let Some(p) = std::env::var("ND_PRESET").ok().and_then(|k| nd_sim::timetable::Preset::from_key(&k)) {
        let terms: Vec<nd_sim::BuildingId> = w.ports.ports.values().filter(|p| p.kind.is_ferry()).filter_map(|p| p.building).collect();
        for b in terms {
            println!("timetable {:?} for {:?}: {:?}", p, b, w.set_line_preset(nd_sim::timetable::LineRef::Ferry(b), p));
        }
    }
    println!("day {} pop {}", w.clock.day(), w.citizens.len());
    summary(&w);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    println!("day | carried riders loads lorries missed moved | pier peak (hour) aboard peak | islanders working-off-island");
    for _ in 0..days {
        let (mut peak, mut peak_h, mut aboard) = (0usize, 0u32, 0usize);
        let mut by_hour = [0usize; 24];
        let mut last = (0, 0, 0, 0, 0, 0, 0);
        for _ in 0..1440u32 {
            w.tick(&mut minds);
            if w.clock.minute_of_day() % 15 == 0 {
                let pier = w.citizens.values().filter(|c| matches!(c.activity, Activity::Waiting { .. }) && matches!(c.transit.map(|t| t.leg), Some(TransitLeg::Ferry { .. }))).count();
                let on = w.citizens.values().filter(|c| matches!(c.activity, Activity::OnFerry { .. })).count();
                let h = w.clock.hour() as usize;
                by_hour[h] = by_hour[h].max(pier + on);
                if pier > peak {
                    peak = pier;
                    peak_h = h as u32;
                }
                aboard = aboard.max(on);
            }
            if std::env::var("ND_TRACE").is_ok() && w.clock.minute_of_day() % 30 == 0 && !(1..18).contains(&w.clock.hour()) {
                let mut acts: std::collections::BTreeMap<&str, u32> = Default::default();
                for c in w.citizens.values() {
                    let Some(k) = c.home.and_then(|h| w.island_of(h)) else { continue };
                    if c.inside.is_some_and(|b| w.island_of(b) == Some(k)) {
                        continue;
                    }
                    *acts.entry(c.activity.label()).or_default() += 1;
                }
                let ships: Vec<String> = w.ports.ships.values().filter(|s| s.kind.is_ferry()).map(|s| format!("{:.0}/{:.0}{}{} p{}", s.s, s.berth_s, if s.docked { "d" } else { "" }, s.dwell, s.passengers)).collect();
                println!("  {} islanders off-island {:?} ships {:?}", w.clock.format_hhmm(), acts, ships);
            }
            if std::env::var("ND_WATCH").is_ok() && w.clock.minute_of_day() % 60 == 0 {
                let name = |b: nd_sim::BuildingId| w.buildings.get(b).map_or("?".to_string(), |x| x.name.clone());
                let ships: Vec<String> = w.ports.ships.values().filter(|s| s.kind.is_big()).map(|s| format!("{}->{}{}", name(s.port), s.to.map_or("off-map".into(), name), if s.docked { "(back)" } else { "" })).collect();
                println!("  {:02}:00 big {} {:?}", w.clock.hour(), ships.len(), ships);
                if w.clock.hour() == 23 {
                    for (id, s) in w.ports.ships.iter().filter(|(_, s)| s.kind.is_big()) {
                        let total = nd_sim::geom::path_length(&s.path);
                        let goal_s = if s.docked { total } else { s.berth_s };
                        let goal = s.pose_at(goal_s).0;
                        let near: Vec<String> = w.ports.ships.iter().filter(|(o, _)| *o != id).map(|(_, o)| format!("{:.0}", o.pose().0.distance(goal))).collect();
                        println!("    {:?} {}->{:?}: s {:.0} berth_s {:.0} total {:.0} docked {} dwell {} home {} pos {:?} goal {:?} others' dist to goal {:?}", s.kind, name(s.port), s.to.map(name), s.s, s.berth_s, total, s.docked, s.dwell, s.home, s.pose().0, goal, near);
                    }
                }
            }
            let s = &w.ports.stats;
            if w.clock.minute_of_day() == 1439 {
                last = (s.own_trips_today, s.ferry_riders_today, s.loads_ferried_today, s.lorries_ferried_today, s.missed_last_boat_today, s.islanders_moved_off_today, s.island_jobs_quit_today);
            }
        }
        let islanders: Vec<_> = w.citizens.values().filter(|c| c.home.is_some_and(|h| w.island_of(h).is_some())).collect();
        let away = islanders.iter().filter(|c| c.work.is_some_and(|j| w.island_of(j).is_none())).count();
        println!("{:>3} | {:>7} {:>6} {:>5} {:>7} {:>6} {:>5} quit {} | {:>4} ({:>2}h) {:>4} | {} {} | pop {}", w.clock.day(), last.0, last.1, last.2, last.3, last.4, last.5, last.6, peak, peak_h, aboard, islanders.len(), away, w.citizens.len());
        println!("     by hour (pier+aboard): {:?}", by_hour);
    }
    summary(&w);
    if let Ok(out) = std::env::var("ND_SAVE_OUT") {
        std::fs::write(&out, w.to_ron().unwrap()).unwrap();
        println!("saved {out}");
    }
}
