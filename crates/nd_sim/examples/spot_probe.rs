//! [r2-traffic] Headless look at one spot of a save at one time: runs
//! `spot_probe <save.ron> <day> <hh:mm> <x> <y> [r=40] [ticks=30] [every=5]`
//! quietly to that game time, then every `every` ticks lists the vehicles
//! within `r` m of (x, y): what they are, where in their lanes, why they
//! stand, and for a crossing they wait at, who is on it. Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::traffic::{LaneKind, StopReason};
use nd_sim::{Vec2, World};

fn look(w: &World, at: Vec2, r: f32) {
    let t = &w.traffic;
    println!("-- day {} {:02}:{:02} tick {}", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.clock.tick);
    let mut ids: Vec<_> = t.cars.iter().filter(|(_, c)| c.pos.distance(at) < r).map(|(id, _)| id).collect();
    ids.sort();
    for id in ids {
        let c = &t.cars[id];
        let lane = c.route.get(c.idx).copied().unwrap_or(0);
        let l = &t.net.lanes[lane as usize];
        let kind = match l.kind {
            LaneKind::Road { segment, forward, index } => format!("road seg {} {} lane {index}", segment.0, if forward { "fwd" } else { "bwd" }),
            LaneKind::Connector { node, .. } => format!("conn node {} turn {}{}{}", node.0, l.turn, if l.uturn { " UTURN" } else { "" }, if l.turnaround { " DEADEND" } else { "" }),
        };
        let why = t.stop_reason(id);
        println!(
            "  {:?} {} at ({:.1},{:.1}) v {:.1} stalled {:.0} hold {:.0} entering {} lateral {:.1} stunt {} lane {lane} {kind} s {:.1}/{:.1} route {}/{} errand {:?} wait {:?} -> {}",
            id,
            w.car_label(id),
            c.pos.x,
            c.pos.y,
            c.v,
            c.stalled,
            c.hold,
            c.entering,
            c.lateral,
            c.in_stunt(),
            c.s,
            l.length,
            c.idx,
            c.route.len(),
            c.errand,
            c.wait,
            why.text(|o| format!("{:?} {}", o, w.car_label(o)))
        );
        if let StopReason::Pedestrians { crossing: Some(k) } | StopReason::Jaywalker { crossing: Some(k) } = why {
            let x = &w.walk.crossings[k as usize];
            println!("     crossing {k} at ({:.1},{:.1}) kind {:?}", x.mid.x, x.mid.y, x.kind);
            for &(cr, p) in w.peds.occupants.iter().filter(|o| o.0 == k) {
                let cz = &w.citizens[p];
                let wk = w.peds.walkers.get(&p);
                println!("       on it: {} at ({:.1},{:.1}) activity {:?} crossing {:?} waiting {:?}", cz.name, cz.pos.x, cz.pos.y, cz.activity, wk.and_then(|w| w.crossing), wk.and_then(|w| w.waiting));
                let _ = cr;
            }
            for (p, wk) in w.peds.walkers.iter().filter(|(_, wk)| wk.waiting.is_some_and(|q| q.0 == k) || wk.crossing.is_some_and(|q| q.0 == k)) {
                let cz = &w.citizens[*p];
                println!("       walker: {} at ({:.1},{:.1}) crossing {:?} waiting {:?}", cz.name, cz.pos.x, cz.pos.y, wk.crossing, wk.waiting);
            }
        }
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let src = &a[1];
    let day: u64 = a[2].parse().unwrap();
    let (hh, mm) = a[3].split_once(':').map(|(h, m)| (h.parse::<u64>().unwrap(), m.parse::<u64>().unwrap())).unwrap();
    let at = Vec2::new(a[4].parse().unwrap(), a[5].parse().unwrap());
    let r: f32 = a.get(6).and_then(|s| s.parse().ok()).unwrap_or(40.0);
    let ticks: u64 = a.get(7).and_then(|s| s.parse().ok()).unwrap_or(30);
    let every: u64 = a.get(8).and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let mut w = World::from_ron(&std::fs::read_to_string(src).unwrap()).expect("load");
    eprintln!("loaded at day {} {:02}:{:02}", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60);
    while w.clock.day() < day || (w.clock.day() == day && w.clock.minute_of_day() < hh * 60 + mm) {
        w.tick(&mut minds);
        if w.clock.minute_of_day() == 0 {
            eprintln!("day {}", w.clock.day());
        }
    }
    for i in 0..=ticks {
        if i % every == 0 {
            look(&w, at, r);
        }
        w.tick(&mut minds);
    }
}
