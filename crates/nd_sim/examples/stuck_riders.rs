//! [r4-hunger] Probe: load a save, run some hours, then find the cyclists
//! who have not moved for an hour: where they stand, what they wait for
//! (the crossing in `peds.riders`, its light and junction), grouped by
//! spot, so a hold that never lets go shows up.
//!
//! cargo run --profile fast -p nd_sim --example stuck_riders -- saves/city.ron 18

use nd_sim::citizens::{Activity, CitizenId, TravelMode};
use nd_sim::World;
use std::collections::BTreeMap;

/// Level crossings shut a long while, and the trains by them.
fn crossings(w: &World) {
    for (k, l) in w.level_crossings().iter().enumerate() {
        if l.closure_seconds < 600.0 || l.phase == nd_sim::level_crossing::CrossingPhase::Open {
            continue;
        }
        println!("level crossing {k} at ({:.0},{:.0}) {:?} shut {:.0} s, eta {:?} occupied {} people waiting {} vehicles waiting {} trains held {}", l.pos.x, l.pos.y, l.phase, l.closure_seconds, l.eta, l.occupied, l.people_waiting, l.vehicles_waiting, l.trains_held);
        for (id, t) in w.rail.trains.iter() {
            let head = t.head();
            if head.distance(l.pos) < 400.0 {
                println!("    train {id:?} {:?} head ({:.0},{:.0}) speed {:.1} held {} dwell {:.0} s {:.0} next_stop {} cars {}", t.kind, head.x, head.y, t.speed, t.held, t.dwell, t.s, t.next_stop, t.cars);
            }
        }
    }
}

/// [r4-hunger] Walkers still for an hour: by spot, with what their walk
/// state says (on a crossing, waiting at one).
fn walkers(w: &World, stuck: &[CitizenId]) {
    let mut spots: BTreeMap<(i32, i32), Vec<CitizenId>> = BTreeMap::new();
    for id in stuck {
        let p = w.citizens[*id].pos;
        spots.entry(((p.x / 20.0).round() as i32 * 20, (p.y / 20.0).round() as i32 * 20)).or_default().push(*id);
    }
    let mut rows: Vec<_> = spots.into_iter().collect();
    rows.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    for (spot, ids) in rows.iter().take(12) {
        let mut st: BTreeMap<String, u32> = BTreeMap::new();
        for id in ids {
            let wk = w.peds.walkers.get(id);
            let c = &w.citizens[*id];
            let act = match &c.activity {
                Activity::Traveling { purpose, path, idx, .. } => format!("{purpose:?} idx {idx}/{}", path.len()),
                _ => String::new(),
            };
            let _ = act;
            *st.entry(format!("crossing {:?} waiting {:?}", wk.and_then(|x| x.crossing).map(|x| x.0), wk.and_then(|x| x.waiting).map(|x| (x.0, w.clock.tick - x.2)))).or_default() += 1;
        }
        let c0 = &w.citizens[ids[0]];
        println!("{:5} walkers at {:?}: {:?}", ids.len(), spot, st.iter().take(6).collect::<Vec<_>>());
        if let Activity::Traveling { path, idx, purpose, .. } = &c0.activity {
            let next: Vec<String> = path.iter().skip(idx.saturating_sub(1)).take(4).map(|p| format!("({:.1},{:.1})", p.x, p.y)).collect();
            println!("        e.g. {:?} pos ({:.1},{:.1}) {:?} idx {}/{} path {} walker {:?}", ids[0], c0.pos.x, c0.pos.y, purpose, idx, path.len(), next.join(" "), w.peds.walkers.get(&ids[0]));
        }
        for (k, x) in w.walk.crossings.iter().enumerate() {
            if x.mid.distance(c0.pos) < 20.0 {
                let sig = w.peds.crossings.get(k).and_then(|i| i.signal);
                println!("        crossing {k} kind {:?} mid ({:.0},{:.0}) signal {:?} walk shown {:?} node {:?}", x.kind, x.mid.x, x.mid.y, sig, sig.map(|(a, b)| w.traffic.peds.walk_shown(a as usize, b as usize)), x.node);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let hours: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(18);
    let mut w = World::read_save(&src).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    println!("at load:");
    crossings(&w);
    let end = w.clock.tick + hours * 60;
    while w.clock.tick < end {
        w.tick(&mut minds);
        w.drain_events();
    }
    let walk = std::env::var("WALK").is_ok();
    let mode = if walk { TravelMode::Walk } else { TravelMode::Bike };
    let riders = |w: &World| -> Vec<(CitizenId, glam::Vec2)> { w.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::Traveling { .. }) && c.mode == mode).map(|(id, c)| (id, c.pos)).collect() };
    let before = riders(&w);
    let t1 = w.clock.tick + 60;
    while w.clock.tick < t1 {
        w.tick(&mut minds);
        w.drain_events();
    }
    crossings(&w);
    let mut stuck: Vec<CitizenId> = Vec::new();
    for (id, p) in &before {
        if w.citizens.get(*id).is_some_and(|c| c.pos.distance(*p) < 3.0 && matches!(c.activity, Activity::Traveling { .. })) {
            stuck.push(*id);
        }
    }
    println!("day {} {:02}:00: riders {} still for an hour {}", w.clock.day(), w.clock.hour(), before.len(), stuck.len());
    if walk {
        walkers(&w, &stuck);
        return;
    }
    // By what they wait at.
    let mut by_cross: BTreeMap<Option<u32>, Vec<CitizenId>> = BTreeMap::new();
    for id in &stuck {
        let wait = w.peds.riders.get(id).and_then(|r| r.waiting).map(|(c, _)| c);
        by_cross.entry(wait).or_default().push(*id);
    }
    let mut rows: Vec<_> = by_cross.into_iter().collect();
    rows.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    for (cr, ids) in rows.iter().take(25) {
        let c0 = &w.citizens[ids[0]];
        let since = w.peds.riders.get(&ids[0]).and_then(|r| r.waiting).map(|(_, t)| w.clock.tick - t);
        print!("{:5} riders at ({:.0},{:.0}) waiting at crossing {:?} for {:?} min", ids.len(), c0.pos.x, c0.pos.y, cr, since);
        if let Some(cr) = cr {
            let x = &w.walk.crossings[*cr as usize];
            let info = w.peds.crossings.get(*cr as usize);
            print!(" | kind {:?} node {:?} seg {:?} signal {:?}", x.kind, x.node, x.segment, info.and_then(|i| i.signal));
            if let Some((k, ap)) = info.and_then(|i| i.signal) {
                let inter = &w.traffic.net.intersections[k as usize];
                let in_phase: Vec<usize> = inter.phases.iter().enumerate().filter(|(_, g)| g.contains(&(ap as usize))).map(|(i, _)| i).collect();
                let conns: Vec<(i8, bool)> = inter.connectors.iter().filter(|(_, a)| *a == ap as usize).map(|(l, _)| (w.traffic.net.lanes[*l as usize].turn, w.traffic.net.lanes[*l as usize].green)).collect();
                print!(" | approaches {} phases {:?} now {} ap {} in phases {:?} connectors (turn, green) {:?} arm segs {:?}", inter.approaches, inter.phases, inter.phase, ap, in_phase, conns, w.roads.node(inter.node).segments.len());
            }
        }
        println!();
        // Where they are going, and their path's next points.
        if let Activity::Traveling { path, idx, purpose, .. } = &c0.activity {
            let next: Vec<String> = path.iter().skip(*idx).take(3).map(|p| format!("({:.0},{:.0})", p.x, p.y)).collect();
            println!("        e.g. {:?} purpose {:?} heading {:.2} idx {}/{} next {}", ids[0], purpose, c0.heading, idx, path.len(), next.join(" "));
        }
        if cr.is_none() {
            let mut spots: BTreeMap<(i32, i32), u32> = BTreeMap::new();
            for id in ids {
                let p = w.citizens[*id].pos;
                *spots.entry(((p.x / 25.0).round() as i32 * 25, (p.y / 25.0).round() as i32 * 25)).or_default() += 1;
            }
            let mut spots: Vec<_> = spots.into_iter().collect();
            spots.sort_by(|a, b| b.1.cmp(&a.1));
            println!("        spots {:?}", &spots[..spots.len().min(10)]);
            let lc: Vec<String> = w.level_crossings().iter().filter(|l| l.pos.distance(c0.pos) < 80.0).map(|l| format!("lc at ({:.0},{:.0}) phase {:?} timer {:.0} reach {:.0}", l.pos.x, l.pos.y, l.phase, l.timer, l.reach)).collect();
            for (k, x) in w.walk.crossings.iter().enumerate() {
                if x.mid.distance(c0.pos) < 25.0 {
                    let info = w.peds.crossings.get(k);
                    let sig = info.and_then(|i| i.signal);
                    let occ = w.peds.occupants.iter().filter(|o| o.0 == k as u32).count();
                    for o in w.peds.occupants.iter().filter(|o| o.0 == k as u32).take(4) {
                        if let Some(c) = w.citizens.get(o.1) {
                            let act = format!("{:?}", c.activity);
                            println!("          occupant {:?} mode {:?} pos ({:.1},{:.1}) prev ({:.1},{:.1}) {} walker {:?}", o.1, c.mode, c.pos.x, c.pos.y, c.prev_pos.x, c.prev_pos.y, &act[..act.len().min(60)], w.peds.walkers.get(&o.1));
                        }
                    }
                    println!("        crossing {k} kind {:?} mid ({:.0},{:.0}) signal {:?} walk shown {:?} occupants {occ} node {:?}", x.kind, x.mid.x, x.mid.y, sig, sig.map(|(a, b)| w.traffic.peds.walk_shown(a as usize, b as usize)), x.node);
                }
            }
            println!("        transit {:?} to_car {} inside {:?} next_decision {} | level crossings near: {:?}", c0.transit.is_some(), c0.to_car.is_some(), c0.inside, c0.next_decision_tick as i64 - w.clock.tick as i64, lc);
        }
    }
}
