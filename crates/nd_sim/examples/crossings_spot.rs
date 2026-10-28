//! [r4-crossings] Load a save and dump the street around a point: nodes with
//! what meets there, crossings and footbridges near it. Not part of the game.
//! `cargo run --profile fast -p nd_sim --example crossings_spot -- save.ron x y [radius]`
use nd_sim::roads::RoadKind;
use nd_sim::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let w = World::read_save(&args[1]).expect("load");
    let x: f32 = args[2].parse().unwrap();
    let y: f32 = args[3].parse().unwrap();
    let rad: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(60.0);
    let c = glam::Vec2::new(x, y);
    let r = &w.roads;
    println!("gestures near:");
    for (id, g) in &w.master.gestures {
        if let nd_sim::plan::Gesture::Road { points, lanes_fwd, lanes_bwd, kind, elevation, curved, .. } = g {
            if points.windows(2).any(|s| nd_sim::roads::project_on_segment(c, s[0], s[1]).1.distance(c) <= rad) || points.iter().any(|p| p.distance(c) <= rad) {
                println!("  g{} {:?} {}/{} curved {} pts {:?} lv {:?}", id.0, kind, lanes_fwd, lanes_bwd, curved, points.iter().map(|p| (p.x.round(), p.y.round())).collect::<Vec<_>>(), elevation);
            }
        }
    }
    for (i, n) in r.nodes.iter().enumerate() {
        if n.pos.distance(c) > rad || !n.segments.iter().any(|&s| r.segment(s).alive) {
            continue;
        }
        let segs: Vec<String> = n.segments.iter().filter(|&&s| r.segment(s).alive).map(|&s| {
            let sg = r.segment(s);
            let other = if sg.a.0 as usize == i { sg.b } else { sg.a };
            let len = r.node(sg.a).pos.distance(r.node(sg.b).pos);
            format!("s{}->n{} {} {}/{} {:.1}m", s.0, other.0, match sg.kind { RoadKind::Street => "St", RoadKind::Path => "Pa", RoadKind::Rail => "Ra" }, sg.lanes_fwd, sg.lanes_bwd, len)
        }).collect();
        println!("n{} ({:.1},{:.1}) lv {:.1} streets {}: {}", i, n.pos.x, n.pos.y, n.level, r.street_degree(nd_sim::roads::NodeId(i as u32)), segs.join(" | "));
    }
    println!("crossings:");
    for (i, k) in w.walk.crossings.iter().enumerate() {
        if k.mid.distance(c) <= rad && k.kind != nd_sim::walk::CrossingKind::Jay {
            println!("  c{} {:?} s{} node {:?} mid ({:.1},{:.1}) closed {} bridged {} button {}", i, k.kind, k.segment.0, k.node.map(|n| n.0), k.mid.x, k.mid.y, k.closed, k.bridged, k.button);
        }
    }
    println!("footbridges:");
    for f in &w.walk.footbridges {
        let k = &w.walk.crossings[f.crossing as usize];
        if k.mid.distance(c) <= rad {
            println!("  over c{} top ({:.1},{:.1})-({:.1},{:.1}) feet ({:.1},{:.1})-({:.1},{:.1})", f.crossing, f.top[0].x, f.top[0].y, f.top[1].x, f.top[1].y, f.foot[0].x, f.foot[0].y, f.foot[1].x, f.foot[1].y);
        }
    }
}
