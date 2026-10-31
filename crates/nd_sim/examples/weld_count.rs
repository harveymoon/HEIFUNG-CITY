//! [r4-flags-sim] Street nodes within 3 m of each other at one level in a
//! save as loaded, and its lots and buildings: `weld_count <save>...`.
use nd_sim::World;
fn main() {
    for path in std::env::args().skip(1) {
        let w = World::read_save(&path).expect("load");
        let r = &w.roads;
        let street = |n: usize| r.nodes[n].segments.iter().any(|&s| { let g = r.segment(s); g.alive && g.is_street() && !g.is_motorway() });
        let mut pairs = Vec::new();
        for a in 0..r.nodes.len() {
            if !street(a) { continue }
            for b in a + 1..r.nodes.len() {
                if street(b) && r.nodes[a].pos.distance(r.nodes[b].pos) < 3.0 && (r.nodes[a].level - r.nodes[b].level).abs() < 0.5 && !r.nodes[b].segments.iter().any(|s| r.nodes[a].segments.contains(s)) {
                    pairs.push((a, b, r.nodes[a].pos));
                }
            }
        }
        let built = w.buildings.len();
        for (id, rt) in w.transit.routes.iter() {
            println!("  line {} stops {} problems {:?}", rt.name, rt.stops.len(), w.line_problems(id).iter().map(|p| p.to_string()).collect::<Vec<_>>());
        }
        println!("{path}: close street node pairs {} lots {} buildings {built} {:?}", pairs.len(), w.lots.len(), pairs.iter().take(8).map(|p| (p.2.x.round(), p.2.y.round())).collect::<Vec<_>>());
    }
}
