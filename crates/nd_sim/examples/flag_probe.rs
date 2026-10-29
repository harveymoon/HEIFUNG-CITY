//! [r4-flags-sim] Headless survey for Harvey's flag review: vehicles
//! standing on top of each other, lorries stuck on a job for hours, and
//! where they are. `flag_probe <save.ron> [ticks] [every]`. Not part of the
//! game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::World;
use std::collections::HashMap;

fn survey(w: &World, verbose: bool) -> (usize, usize) {
    let t = &w.traffic;
    // Stacked: two stopped vehicles whose centres are within 1.5 m.
    let mut grid: HashMap<(i32, i32), Vec<nd_sim::CarId>> = HashMap::new();
    for (id, c) in t.cars.iter() {
        if c.v < 0.3 {
            grid.entry(((c.pos.x / 4.0).floor() as i32, (c.pos.y / 4.0).floor() as i32)).or_default().push(id);
        }
    }
    let mut stacked = 0;
    let mut shown = 0;
    for (id, c) in t.cars.iter() {
        if c.v >= 0.3 {
            continue;
        }
        let (gx, gy) = ((c.pos.x / 4.0).floor() as i32, (c.pos.y / 4.0).floor() as i32);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &o in grid.get(&(gx + dx, gy + dy)).map(|v| v.as_slice()).unwrap_or(&[]) {
                    if o <= id {
                        continue;
                    }
                    let oc = &t.cars[o];
                    if oc.pos.distance(c.pos) < 1.5 {
                        stacked += 1;
                        if verbose && shown < 12 {
                            shown += 1;
                            println!("  STACK {} [{:?} entering {} hold {:.0} lat {:.1} wait {:?}] + {} [{:?} entering {} hold {:.0} lat {:.1} wait {:?}] at ({:.0},{:.0})",
                                w.car_label(id), t.stop_reason(id).key(), c.entering, c.hold, c.lateral, c.wait,
                                w.car_label(o), t.stop_reason(o).key(), oc.entering, oc.hold, oc.lateral, oc.wait, c.pos.x, c.pos.y);
                        }
                    }
                }
            }
        }
    }
    // Lorries on one trip for over 10 hours.
    let mut long = 0;
    for (id, c) in t.cars.iter() {
        if c.cargo.is_none() {
            continue;
        }
        if let Some((el, ex)) = w.car_trip_minutes(id) {
            if el > 600.0 && el > 4.0 * ex {
                long += 1;
                if verbose && long <= 3 {
                    // How much of the lane graph can it reach from here?
                    let mut seen = std::collections::HashSet::new();
                    let mut stack = vec![c.route[c.idx]];
                    while let Some(l) = stack.pop() {
                        if seen.insert(l) {
                            let lane = &t.net.lanes[l as usize];
                            stack.extend(lane.next.iter().copied());
                            stack.extend(lane.left.iter().copied());
                            stack.extend(lane.right.iter().copied());
                        }
                    }
                    let cg = c.cargo.unwrap();
                    let bname = |b: nd_sim::BuildingId| w.buildings.get(b).map_or("-".to_string(), |x| format!("{} op {} derelict {}", x.name, x.operating(), x.is_derelict()));
                    if std::env::var("POCKET").is_ok() {
                        for &l in &seen {
                            let lane = &t.net.lanes[l as usize];
                            let seg = match lane.kind { nd_sim::traffic::LaneKind::Road { segment, .. } => Some(segment), _ => None };
                            let sd = seg.map(|s| { let g = w.roads.segment(s); format!("seg {:?} {:?} fwd {} bwd {} ramp {} car_only {:?} ring {} a {:?}({:.0},{:.0}) segs {} b {:?}({:.0},{:.0}) segs {}", s, g.kind, g.lanes_fwd, g.lanes_bwd, g.ramp, g.car_only, g.ring, g.a, w.roads.node(g.a).pos.x, w.roads.node(g.a).pos.y, w.roads.node(g.a).segments.len(), g.b, w.roads.node(g.b).pos.x, w.roads.node(g.b).pos.y, w.roads.node(g.b).segments.len()) });
                            if let Some(s) = seg {
                                let g = w.roads.segment(s);
                                for n in [g.a, g.b] {
                                    for &o in &w.roads.node(n).segments {
                                        let og = w.roads.segment(o);
                                        println!("        node {:?} seg {:?} alive {} {:?} street {} fwd {} bwd {} level {:.1} lanes {:?}", n, o, og.alive, og.kind, og.is_street(), og.lanes_fwd, og.lanes_bwd, w.roads.node(n).level, t.net.segment_lanes.get(&o));
                                    }
                                }
                            }
                            println!("      lane {l} len {:.0} next {:?} prev {:?} {:?} {}", lane.length, lane.next, lane.prev, lane.kind, sd.unwrap_or_default());
                        }
                    }
                    println!("    reach {} of {} lanes; lane {:?} kind {:?}; from {} to {}", seen.len(), t.net.lanes.len(), c.route[c.idx], t.net.lanes[c.route[c.idx] as usize].kind, bname(cg.from), bname(cg.to));
                }
                if verbose && long <= 10 {
                    println!("  LONG {} {:.0}/{:.0} min [{}] entering {} hold {:.0} lat {:.1} wait {:?} route {} idx {} at ({:.0},{:.0}) errand {:?}",
                        w.car_label(id), el, ex, t.stop_reason(id).key(), c.entering, c.hold, c.lateral, c.wait, c.route.len(), c.idx, c.pos.x, c.pos.y, c.errand);
                }
            }
        }
    }
    (stacked, long)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&args[1]).expect("load");
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(240);
    let every: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(60);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let dead = w.traffic.net.lanes.iter().filter(|l| l.next.is_empty() && matches!(l.kind, nd_sim::traffic::LaneKind::Road { .. })).count();
    println!("road lanes leading nowhere: {dead}; trap lanes {}", w.traffic.net.trap.iter().filter(|&&t| t).count());
    let (s, l) = survey(&w, true);
    println!("t=0 day {} {:02}:{:02} cars {} stacked {s} long_lorries {l}", w.clock.day(), w.clock.hour(), w.clock.minute(), w.traffic.cars.len());
    let (mut st, mut lt, mut n) = (0, 0, 0);
    for i in 1..=ticks {
        w.tick(&mut minds);
        if i % every == 0 {
            let (s, l) = survey(&w, i == ticks);
            st += s;
            lt += l;
            n += 1;
            println!("t={i} day {} {:02}:{:02} cars {} stacked {s} long_lorries {l}", w.clock.day(), w.clock.hour(), w.clock.minute(), w.traffic.cars.len());
        }
    }
    println!("MEAN stacked {:.1} long_lorries {:.1}", st as f32 / n.max(1) as f32, lt as f32 / n.max(1) as f32);
}
