//! Load a save, step a tick, and say why the car nearest a point stands
//! "for people on a crossing": which crossings hold its lanes, of what
//! kind, and who is on them.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::World;
use glam::Vec2;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut w = World::from_ron(&std::fs::read_to_string(&a[1]).unwrap()).expect("load");
    let at = Vec2::new(a[2].parse().unwrap(), a[3].parse().unwrap());
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    w.tick(&mut minds);
    let t = &w.traffic;
    let (id, car) = t.cars.iter().min_by(|x, y| x.1.pos.distance(at).total_cmp(&y.1.pos.distance(at))).unwrap();
    println!("car {:?} at ({:.1},{:.1}) v {:.1} wait {:?} why {:?}", id, car.pos.x, car.pos.y, car.v, car.wait, t.stop_reason(id).text(|c| format!("{c:?}")));
    let mut kinds = std::collections::BTreeMap::new();
    for &(cr, _) in &w.peds.occupants {
        let k = w.walk.crossings.get(cr as usize).map(|c| format!("{:?}", c.kind)).unwrap_or_default();
        *kinds.entry(k).or_insert(0) += 1;
    }
    println!("occupied crossings by kind (city-wide): {kinds:?}");
    for k in car.idx..(car.idx + 3).min(car.route.len()) {
        let l = car.route[k];
        for &(cr, cid) in w.peds.occupants.iter() {
            let Some(info) = w.peds.crossings.get(cr as usize) else { continue };
            if info.lanes.iter().any(|&(ll, _)| ll == l) {
                let c = &w.walk.crossings[cr as usize];
                let p = w.citizens.get(cid);
                println!("  lane {l}: crossing {cr} kind {:?} mid ({:.0},{:.0}) occupant {:?} at {:?} visible {:?} activity {:?}", c.kind, c.mid.x, c.mid.y, cid,
                    p.map(|p| (p.pos.x.round(), p.pos.y.round())), p.map(|p| p.is_visible()), p.map(|p| format!("{:?}", p.activity).chars().take(60).collect::<String>()));
            }
        }
    }
    println!("crossings within 40 m:");
    for (i, c) in w.walk.crossings.iter().enumerate() {
        if c.mid.distance(at) < 40.0 {
            let ring = w.roads.ring_of_segment(c.segment).is_some();
            println!("  {i} kind {:?} mid ({:.0},{:.0}) on ring {ring} node {:?} occupied now {}", c.kind, c.mid.x, c.mid.y, c.node, w.peds.occupants.iter().any(|o| o.0 as usize == i));
        }
    }
}
