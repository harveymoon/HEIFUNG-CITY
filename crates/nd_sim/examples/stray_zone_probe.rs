//! [stray-zones] Probe: load a save and list the parcels the master plan
//! zones that have no lot of that use (the plan hologram's raised fills
//! that never go away), with what stands in the way: a column on the
//! ground, a deck too low over it, a structure's footprint. Then rebuild
//! from the master plan and list them again. Not part of the game.
//!
//! cargo run --profile fast -p nd_sim --example stray_zone_probe -- <save.ron>

use nd_sim::World;

fn list(w: &World, label: &str) {
    let planned = w.planned_zoning().to_vec();
    let mut n = 0;
    for (i, p) in w.parcels.parcels.iter().enumerate() {
        let Some(&want) = planned.get(i) else { break };
        // A parcel still naming a lot that is gone counts as not built.
        let built = p.lot.is_some_and(|l| w.lots.contains_key(l));
        if want.is_none() || (want == p.land_use && built) {
            continue;
        }
        if p.lot.and_then(|l| w.lots.get(l)).is_some_and(|l| w.is_city_ground(l)) {
            continue;
        }
        n += 1;
        let area = nd_sim::geom::polygon_area(&p.polygon).abs();
        let lot = p.lot.map(|l| (w.lots.contains_key(l), w.lots.get(l).map(|x| x.land_use)));
        println!(
            "   {i}: want {:?} now {:?} lot {:?} at ({:.0},{:.0}) {:.0} m² rough {} path_frontage {} pillars {} deck_cap {:?} refusal {:?}",
            want,
            p.land_use,
            lot,
            p.center.x,
            p.center.y,
            area,
            p.rough,
            p.path_frontage,
            w.pillars_on(&p.polygon),
            w.deck_cap_of(&p.polygon),
            w.parcel_refusal(i, want),
        );
    }
    println!("{label}: {n} parcels planned but not built as planned; lots {}", w.lots.len());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).expect("save path");
    let mut w = World::read_save(src).expect("load");
    list(&w, "loaded");
    w.rebuild_from_plan();
    list(&w, "rebuilt");
    let text = w.to_ron().unwrap();
    let back = World::from_ron(&text).unwrap();
    list(&back, "saved and loaded");
}
