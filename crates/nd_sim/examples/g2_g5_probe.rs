//! [fix/roads-tools] Probe for flag G5: Harvey's civic zoning round Market
//! Circus (taken from the save after the flag) put back on the save before
//! it, implemented; how many civic lots come out as unusable ground. Not
//! part of the game. `g2_g5_probe <before.ron> <after.ron>`
use nd_sim::{Gesture, LandUse, World};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut w = World::from_ron(&std::fs::read_to_string(&a[1]).unwrap()).expect("load");
    if a.get(2).map(String::as_str) == Some("rebuild") {
        let count = |w: &World| w.lots.values().filter(|l| l.land_use == LandUse::Civic && l.landlocked && l.center.distance(nd_sim::Vec2::new(-600.0, 0.0)) < 110.0).count();
        let civ = |w: &World| w.lots.values().filter(|l| l.land_use == LandUse::Civic && l.center.distance(nd_sim::Vec2::new(-600.0, 0.0)) < 110.0).count();
        let pend0 = w.planned_zoning().iter().zip(&w.parcels.parcels).filter(|(z, p)| **z != p.land_use).count();
        println!("before: {} civic lots round the circus, {} unusable; planned other than built {pend0}", civ(&w), count(&w));
        let near = |w: &World| w.planned_zoning().iter().zip(&w.parcels.parcels).filter(|(z, p)| **z != p.land_use && p.center.distance(nd_sim::Vec2::new(-600.0, 0.0)) < 110.0).count();
        println!("  pending round the circus {}", near(&w));
        w.rebuild_from_plan();
        let pend = w.planned_zoning().iter().zip(&w.parcels.parcels).filter(|(z, p)| **z != p.land_use).count();
        println!("after a rebuild: {} civic lots, {} unusable; parcels planned other than built: {pend}, round the circus {}", civ(&w), count(&w), near(&w));
        return;
    }
    let after = World::from_ron(&std::fs::read_to_string(&a[2]).unwrap()).expect("load");
    w.project = None;
    let mut n = 0;
    for (id, g) in &after.master.gestures {
        if w.master.gestures.contains_key(id) {
            continue;
        }
        if let Gesture::ParcelZone { land_use: Some(LandUse::Civic), parcels, .. } = g {
            println!("civic stroke {id:?}: {} parcels", parcels.len());
            w.add_gesture(g.clone());
            n += 1;
        }
    }
    if n == 0 {
        println!("no civic stroke");
        return;
    }
    let planned = w.planned_zoning().iter().filter(|z| **z == Some(LandUse::Civic)).count();
    println!("planned civic parcels: {planned}");
    let built = w.parcels.parcels.iter().filter(|p| p.land_use == Some(LandUse::Civic)).count();
    w.try_implement().expect("implemented");
    let civic: Vec<_> = w.lots.values().filter(|l| l.land_use == LandUse::Civic && w.zones.contains_key(l.zone)).collect();
    let ll = civic.iter().filter(|l| l.landlocked).count();
    println!("civic parcels before {built}; civic lots now {} of which unusable (landlocked) {ll}", civic.len());
    for l in civic.iter().filter(|l| l.landlocked).take(30) {
        let parcels: Vec<_> = w.parcels.parcels.iter().filter(|p| p.lot.is_some_and(|x| w.lots.get(x).is_some_and(|y| std::ptr::eq(y, *l)))).collect();
        println!("  unusable lot at ({:.0},{:.0}) area {:.0} parcels {} path_frontage {:?} rough {:?}", l.center.x, l.center.y, l.area(), parcels.len(), parcels.iter().map(|p| p.path_frontage).collect::<Vec<_>>(), parcels.iter().map(|p| p.rough).collect::<Vec<_>>());
    }
}
