//! [r4-crossings] Where laying a save's plan again changes its roads: the
//! nodes the save has that the relaid network lacks, and the reverse. Not
//! part of the game. `relay_diff <save.ron>`
use nd_sim::World;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let text = nd_sim::world::save_text(&std::fs::read(&a[1]).unwrap()).unwrap();
    let raw: World = ron::from_str(&text).expect("parse");
    let mut w = World::from_ron(&text).expect("load");
    if std::env::var("REBUILD").is_ok() {
        w.rebuild_from_plan();
    }
    let live = |w: &World| -> Vec<(glam::Vec2, usize)> {
        w.roads.nodes.iter().filter(|n| n.segments.iter().any(|&s| w.roads.segment(s).alive)).map(|n| (n.pos, n.segments.iter().filter(|&&s| w.roads.segment(s).alive).count())).collect()
    };
    let (old, new) = (live(&raw), live(&w));
    let gone: Vec<_> = old.iter().filter(|(p, _)| !new.iter().any(|(q, _)| q.distance(*p) < 0.05)).collect();
    let added: Vec<_> = new.iter().filter(|(p, _)| !old.iter().any(|(q, _)| q.distance(*p) < 0.05)).collect();
    println!("nodes {} -> {}, gone {}, added {}", old.len(), new.len(), gone.len(), added.len());
    for (p, d) in gone.iter().take(40) {
        println!("  gone ({:.1},{:.1}) deg {d}", p.x, p.y);
    }
    for (p, d) in added.iter().take(40) {
        println!("  added ({:.1},{:.1}) deg {d}", p.x, p.y);
    }
    for (id, b) in raw.buildings.iter() {
        if !w.buildings.contains_key(id) {
            let at = raw.lots.get(b.lot).map(|l| l.center).unwrap_or_default();
            println!("  building lost {id:?} {:?} at ({:.0},{:.0})", b.land_use, at.x, at.y);
        }
    }
    println!("buildings {} -> {}, lots {} -> {}, cars {} -> {}", raw.buildings.len(), w.buildings.len(), raw.lots.len(), w.lots.len(), raw.traffic.cars.len(), w.traffic.cars.len());
}
