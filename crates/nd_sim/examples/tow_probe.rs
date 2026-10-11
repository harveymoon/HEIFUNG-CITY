//! Headless tow probe: every tow truck (parking tows, wreck recovery) on
//! the road, how long it has stood and why (`tow_probe <save.ron> [ticks]
//! [every]`). Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::vehicle::{BodyType, Errand};
use nd_sim::World;

fn report(w: &World) {
    let t = &w.traffic;
    println!("tick {} day {} {:02}:{:02} jobs {} wrecks {} tows {} moves {} watchdog rerouted {} cleared {} pound detours {} u-turns {}", w.clock.tick, w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.enforcement.jobs.len(), w.driving.wrecks.len(), w.driving.tows.len(), w.driving.moves.len(), w.driving.watchdog_rerouted_total, w.driving.watchdog_cleared_total, w.enforcement.stats.pound_detours_total, w.enforcement.stats.u_turns_total);
    for (id, c) in t.cars.iter() {
        let tow = matches!(c.model.body, BodyType::TowTruck | BodyType::HeavyTow) || matches!(c.errand, Errand::Tow(_) | Errand::Recover(_));
        if !tow {
            continue;
        }
        let lane = c.route.get(c.idx).copied();
        let job = if let Errand::Tow(j) = c.errand { w.enforcement.job(j).map(|j| format!("{:?} reason {:?} since {}", j.state, j.reason, j.since)) } else { None };
        let wreck = if let Errand::Recover(car) = c.errand { w.driving.wrecks.iter().find(|x| x.car == car).map(|x| format!("wreck since {} due {} sent {} flipped {} righted {}", x.since, x.due, x.sent, x.flipped, x.righted_at)) } else { None };
        let hooked = w.driving.tows.iter().any(|x| x.truck == id);
        let moving = w.driving.moves.iter().find(|m| m.truck == id).map(|m| format!("{:?} since {}", m.then, m.since));
        if c.v < 0.1 && c.idx + 1 >= c.route.len() {
            if let Some(l) = lane {
                let ln = &t.net.lanes[l as usize];
                let company = if let Errand::Tow(j) = c.errand { w.enforcement.job(j).and_then(|j| j.company) } else { None };
                let stop = company.and_then(|b| w.buildings.get(b)).and_then(|b| w.lots.get(b.lot)).map(|lot| (lot.car_segment, lot.front_segment, lot.car_point, lot.entrance()));
                let goal = stop.and_then(|(cs, fs, cp, e)| match cs { Some(s) => t.net.nearest_lane_on(s, cp), None => t.net.nearest_lane_on(fs, e) });
                let r = goal.and_then(|(g, se, _)| t.net.route_lanes(l, c.s, g, se));
                let seg = match ln.kind { nd_sim::traffic::LaneKind::Road { segment, .. } => Some(segment), _ => None };
                let sinfo = seg.map(|s| { let x = w.roads.segment(s); format!("seg {:?} alive {} ramp {} hw {} kind {:?} lanes {}/{}", s, x.alive, x.ramp, x.is_highway(), x.kind, x.lanes_fwd, x.lanes_bwd) });
                println!("    LANE {l} len {:.1} kind {:?} next {:?} highway {} ramp {} | {:?} | company {:?} goal {:?} route {:?}", ln.length, ln.kind, ln.next, ln.highway, ln.ramp, sinfo, company, goal, r.map(|r| r.0.len()));
            }
        }
        println!(
            "  {:?} at ({:.0},{:.0}) v{:.1} stalled {:.0} hold {:.0} wait {:?} entering {} lane {:?} s {:.1}/{:.1} idx {}/{} errand {:?} hooked {} job {:?} wreck {:?} move {:?} why {}",
            id, c.pos.x, c.pos.y, c.v, c.stalled, c.hold, c.wait, c.entering, lane, c.s, c.s_end, c.idx, c.route.len(), c.errand, hooked, job, wreck, moving,
            t.stop_reason(id).text(|x| format!("{x:?}"))
        );
    }
}

fn jobs(w: &World) {
    let mut by: std::collections::BTreeMap<String, usize> = Default::default();
    for j in &w.enforcement.jobs {
        let Some(c) = w.vehicles.cars.get(j.car) else { continue };
        let Some(sp) = c.spot() else { continue };
        let seg = sp.segment.and_then(|s| w.roads.try_segment(s)).map(|x| format!("ramp {} hw {} motorway {}", x.ramp, x.is_highway(), x.is_motorway()));
        let k = format!("{:?} {:?} {:?}", j.state, sp.kind, seg);
        *by.entry(k).or_default() += 1;
        if matches!(j.state, nd_sim::enforcement::TowState::Waiting { .. }) && seg.as_deref().is_some_and(|s| s.contains("true")) {
            println!("   job {} car at ({:.0},{:.0}) {:?} {:?} owner parked since {}", j.id, sp.pos.x, sp.pos.y, sp.kind, seg, sp.since);
        }
    }
    for (k, n) in by {
        println!("  jobs {n}: {k}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save");
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let every: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(60);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let mut w = World::from_ron(&std::fs::read_to_string(&src).unwrap()).expect("load");
    report(&w);
    jobs(&w);
    for i in 0..ticks {
        w.tick(&mut minds);
        if (i + 1) % every == 0 {
            report(&w);
        }
    }
}
