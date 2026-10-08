//! List road segments and walk edges near a point. Not part of the game.
use nd_sim::World;
use glam::Vec2;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = World::from_ron(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let p = Vec2::new(a[2].parse().unwrap(), a[3].parse().unwrap());
    let r: f32 = a[4].parse().unwrap();
    for (sid, s) in w.roads.alive_segments() {
        let (x, y) = w.roads.segment_points(sid);
        let d = nd_sim::geom::project_on_segment(p, x, y).1.distance(p);
        if d < r {
            println!("seg {} {:?} fwd {} bwd {} ramp {} hw {} tunnel {} nodes {}->{} ({:.1},{:.1})->({:.1},{:.1}) hw/2 {:.1} lvl a {:.1} b {:.1}", sid.0, s.kind, s.lanes_fwd, s.lanes_bwd, s.ramp, s.is_highway(), s.tunnel, s.a.0, s.b.0, x.x, x.y, y.x, y.y, s.half_width(), w.roads.node(s.a).level, w.roads.node(s.b).level);
        }
    }
    // walk edges that run over a motorway carriageway
    let g = &w.walk;
    for (i, n) in g.nodes.iter().enumerate() {
        for e in &g.adj[i] {
            if (e.to as usize) < i { continue; }
            let m = g.nodes[e.to as usize].pos;
            if nd_sim::geom::project_on_segment(p, n.pos, m).1.distance(p) > r { continue; }
            for k in 0..=8 {
                let q = n.pos.lerp(m, k as f32 / 8.0);
                if let Some((sid, _, qq)) = w.roads.nearest_on_network(q) {
                    let s = w.roads.segment(sid);
                    if s.fronts_nothing() && qq.distance(q) < s.half_width() {
                        println!("walk edge {i}->{} cross_kind {:?} ({:.1},{:.1})->({:.1},{:.1}) interior {} cross {} raised {} over seg {}", e.to, (e.cross != u32::MAX).then(|| g.crossings[e.cross as usize].kind), n.pos.x, n.pos.y, m.x, m.y, e.interior, e.cross, e.raised, sid.0);
                        break;
                    }
                }
            }
        }
    }
}
