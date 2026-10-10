//! Lane choice probe: cars standing at the end of their lane waiting to
//! change into the lane beside for their turn (the stop-line 90-degree
//! change), whether another way exists from where they stand, and the
//! lanes there. `lane_choice_probe <save.ron> [ticks]`. Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::traffic::Wait;
use nd_sim::World;

fn report(w: &World, label: &str) {
    let t = &w.traffic;
    let net = &t.net;
    let mut n = 0;
    let mut no_way = 0;
    let mut lines = Vec::new();
    for (id, c) in t.cars.iter() {
        let Some(&next) = c.route.get(c.idx + 1) else { continue };
        let here = c.route[c.idx];
        let lane = &net.lanes[here as usize];
        if !lane.is_beside(next) || c.v > 0.5 || c.stalled < 15.0 {
            continue;
        }
        let to_j = net.to_junction(&c.route, c.idx, c.s, 40.0);
        if to_j > 10.0 {
            continue;
        }
        n += 1;
        let goal = *c.route.last().unwrap();
        let alt = net.route_avoiding(here, c.s, goal, c.s_end, None, Some((here, next)));
        if alt.is_none() {
            no_way += 1;
        }
        if lines.len() < 12 {
            let nexts: Vec<(i8, bool)> = lane.next.iter().map(|&x| (net.lanes[x as usize].turn, net.lanes[x as usize].permitted)).collect();
            lines.push(format!(
                "  {id:?} lane {here} len {:.1} s {:.1} arrows {} next {:?} -> beside {next} then {:?} wait {:?} stalled {:.0} alt {} prev {:?} pos ({:.0},{:.0})",
                lane.length,
                c.s,
                nd_sim::traffic::arrows_label(lane.arrows),
                nexts,
                c.route.get(c.idx + 2).map(|&x| net.lanes[x as usize].turn),
                c.wait,
                c.stalled,
                alt.as_ref().map_or("NONE".to_string(), |r| format!("{} lanes", r.0.len())),
                if c.idx > 0 { Some(c.route[c.idx - 1]) } else { None },
                c.pos.x,
                c.pos.y
            ));
        }
    }
    let lc = t.cars.values().filter(|c| c.wait == Wait::LaneChange).count();
    println!("{label}: day {} {:02}h vehicles {} at-stop-line changes {n} (no other way {no_way}) waiting lanechange {lc}", w.clock.day(), w.clock.hour(), t.cars.len());
    for l in lines {
        println!("{l}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).unwrap();
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(600);
    let mut w = World::from_ron(&text).expect("load");
    drop(text);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    if let Ok(l) = std::env::var("LANE") {
        let net = &w.traffic.net;
        let l: u32 = l.parse().unwrap();
        let reach = |from: u32| {
            let mut seen = std::collections::HashSet::new();
            let mut st = vec![from];
            while let Some(x) = st.pop() {
                if !seen.insert(x) { continue; }
                let ln = &net.lanes[x as usize];
                for &n in ln.next.iter().filter(|&&n| net.lanes[n as usize].permitted) { st.push(n); }
                for n in [ln.left, ln.right].into_iter().flatten() { st.push(n); }
            }
            seen
        };
        let lane = &net.lanes[l as usize];
        println!("lane {l} kind {:?} len {:.1} left {:?} right {:?} prev {:?}", lane.kind, lane.length, lane.left, lane.right, lane.prev);
        for &c in &lane.next {
            let cl = &net.lanes[c as usize];
            let ex = cl.next.first().copied();
            println!("  conn {c} turn {} permitted {} -> {:?} kind {:?} reach {}", cl.turn, cl.permitted, ex, ex.map(|e| &net.lanes[e as usize].kind), ex.map_or(0, |e| reach(e).len()));
        }
        println!("  reach from lane itself (incl changes) {}, all lanes {}", reach(l).len(), net.lanes.len());
        for &p in &lane.prev { let pl=&net.lanes[p as usize]; println!("  prev {p} turn {} from {:?}", pl.turn, pl.prev); }
        return;
    }
    report(&w, "loaded");
    for i in 0..ticks {
        w.tick(&mut minds);
        if (i + 1) % 120 == 0 {
            report(&w, &format!("+{}", i + 1));
        }
    }
}
