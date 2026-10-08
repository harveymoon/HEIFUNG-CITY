//! Load a save and dump the road topology around a point: every node within
//! the radius with its level and height, and every piece touching one, with
//! its lanes, flags and the gesture it came from. Not part of the game.
//! `cargo run --profile fast -p nd_sim --example junction_dump -- save.ron x y [radius]`
use nd_sim::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::from_ron(&std::fs::read_to_string(&args[1]).unwrap()).expect("load");
    if std::env::var("REBUILD").is_ok() {
        w.rebuild_from_plan();
    }
    let x: f32 = args[2].parse().unwrap();
    let y: f32 = args[3].parse().unwrap();
    let rad: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(120.0);
    let c = glam::Vec2::new(x, y);
    let r = &w.roads;
    println!("gestures near:");
    for (id, g) in &w.master.gestures {
        if let nd_sim::plan::Gesture::Road { points, lanes_fwd, lanes_bwd, kind, elevation, curved } = g {
            if points.iter().any(|p| p.distance(c) <= rad) || points.windows(2).any(|s| nd_sim::roads::project_on_segment(c, s[0], s[1]).1.distance(c) <= rad) {
                println!("  g{} {:?} {}/{} curved {} pts {:?} lv {:?}", id.0, kind, lanes_fwd, lanes_bwd, curved, points.iter().map(|p| (p.x.round(), p.y.round())).collect::<Vec<_>>(), elevation);
            }
        }
    }
    let mut nodes: Vec<usize> = Vec::new();
    for (i, n) in r.nodes.iter().enumerate() {
        if n.pos.distance(c) <= rad && n.segments.iter().any(|&s| r.segment(s).alive) {
            nodes.push(i);
        }
    }
    for &i in &nodes {
        let n = &r.nodes[i];
        let segs: Vec<String> = n.segments.iter().filter(|&&s| r.segment(s).alive).map(|&s| {
            let sg = r.segment(s);
            let other = if sg.a.0 as usize == i { sg.b } else { sg.a };
            let dir = if sg.a.0 as usize == i { "out" } else { "in" };
            format!("s{}->n{} {}/{} {}{}{}{}{}{}", s.0, other.0, sg.lanes_fwd, sg.lanes_bwd, dir,
                if sg.is_highway() { " HWY" } else { "" }, if sg.ramp { " RAMP" } else { "" }, if sg.viaduct { " VIA" } else { "" }, if sg.tunnel { " TUN" } else { "" }, if sg.kind != nd_sim::roads::RoadKind::Street { format!(" {:?}", sg.kind) } else { String::new() })
        }).collect();
        let h = n.segments.iter().find(|&&s| r.segment(s).alive).map(|&s| w.road_height(s, if r.segment(s).a.0 as usize == i { 0.0 } else { 1.0 })).unwrap_or(0.0);
        println!("n{} ({:.1},{:.1}) level {:.1} h {:.1} ground {:.1} deg {}: {}", i, n.pos.x, n.pos.y, n.level, h, w.ground_height(n.pos), segs.len(), segs.join(" | "));
    }
    println!("grade crossings near:");
    for cr in r.grade_crossings() {
        if cr.at.distance(c) <= rad {
            let u = r.segment(cr.upper.0);
            let l = r.segment(cr.lower.0);
            println!("  at ({:.1},{:.1}) upper s{} {}/{} lvl {:.1} over lower s{} {}/{} lvl {:.1}", cr.at.x, cr.at.y, cr.upper.0 .0, u.lanes_fwd, u.lanes_bwd, r.segment_level(cr.upper.0, cr.upper.1), cr.lower.0 .0, l.lanes_fwd, l.lanes_bwd, r.segment_level(cr.lower.0, cr.lower.1));
        }
    }
}
