//! [fix/roads-tools] Flag G5: civic zoning painted over a block's backland
//! came out as lots of "unusable ground" (levelled for nothing); a lot kept
//! from before is cut again to take in backland painted behind it.
//! [land-assembly] Since "zone any parcel" (Harvey 2026-10-15) backland
//! nothing in front takes in is zoned all the same: a landlocked lot that
//! nothing builds on alone, until a company on the plot in front joins it
//! to its site (G5 left it unzoned and took it out of the stroke).

use super::*;

fn block() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-200.0, 300.0), Vec2::new(200.0, 300.0)], 1, 1);
    w
}

fn in_block(w: &World) -> Vec<usize> {
    (0..w.parcels.len()).filter(|&i| { let c = w.parcels.parcels[i].center; c.y > 0.0 && c.y < 300.0 && c.x.abs() < 190.0 }).collect()
}

fn paint(w: &mut World, ids: &[usize]) {
    w.project = None;
    w.paint_parcels(ids, Some(LandUse::Civic), None);
    w.try_implement().expect("implemented");
}

fn planned_is_built(w: &World) {
    let planned = w.planned_zoning();
    for (i, p) in w.parcels.parcels.iter().enumerate() {
        assert_eq!(planned[i], p.land_use, "parcel at {} planned other than built", p.center);
    }
}

#[test]
fn civic_over_a_whole_block_zones_all_of_it() {
    let mut w = block();
    let ids = in_block(&w);
    let back = ids.iter().filter(|&&i| !w.parcels.parcels[i].path_frontage).count();
    assert!(back > 0, "the block has backland");
    paint(&mut w, &ids);
    planned_is_built(&w);
    assert!(w.lots.values().any(|l| l.land_use == LandUse::Civic && !l.landlocked), "the fronts are civic");
    for &i in &in_block(&w) {
        let p = &w.parcels.parcels[i];
        if !p.rough {
            assert_eq!(p.land_use, Some(LandUse::Civic), "every parcel of the block is zoned, backland too: {}", p.center);
        }
    }
}

#[test]
fn backland_nothing_could_take_in_is_a_landlocked_lot_nothing_builds_on_alone() {
    let mut w = block();
    let ids = in_block(&w);
    let (front, back): (Vec<usize>, Vec<usize>) = ids.iter().partition(|&&i| w.parcels.parcels[i].path_frontage);
    let keys: Vec<_> = back.iter().map(|&i| w.parcels.parcels[i].key).collect();
    paint(&mut w, &front);
    let civic = w.lots.values().filter(|l| l.land_use == LandUse::Civic && !l.landlocked).count();
    let back: Vec<usize> = keys.iter().filter_map(|k| w.parcels.find(k)).collect();
    // Here the fronts are narrower than the backland behind them: no lot
    // takes it in, and the Zone tool zones it all the same.
    for &i in &back {
        assert_eq!(w.parcel_refusal(i, Some(LandUse::Civic)), None);
        assert_eq!(w.parcel_refusal(i, Some(LandUse::Commercial)), None, "any use");
    }
    w.project = None;
    w.add_gesture(Gesture::ParcelZone { parcels: keys.clone(), land_use: Some(LandUse::Civic), industry: None });
    w.try_implement().unwrap();
    planned_is_built(&w);
    assert_eq!(w.lots.values().filter(|l| l.land_use == LandUse::Civic && !l.landlocked).count(), civic, "the front lots stay as they were");
    let locked: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Civic && l.landlocked).map(|(id, _)| id).collect();
    assert!(!locked.is_empty(), "the backland is a landlocked lot of its own");
    assert!(w.master.gestures.values().any(|g| matches!(g, Gesture::ParcelZone { parcels, .. } if parcels.iter().any(|k| keys.contains(k)))), "the stroke keeps the backland");
    for l in locked {
        assert!(!w.site_clear(l, SiteRule::STREET), "ordinary growth never builds on backland alone");
    }
}
