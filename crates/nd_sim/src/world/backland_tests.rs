//! [fix/roads-tools] Flag G5: civic zoning painted over a block's backland
//! came out as lots of "unusable ground" (levelled for nothing). Backland
//! is painted only where a lot in front could take it in, a lot kept from
//! before is cut again to take it in, and backland nothing takes in is
//! left unzoned rather than made a landlocked lot.

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

fn no_unusable_civic(w: &World) {
    for l in w.lots.values().filter(|l| l.land_use == LandUse::Civic) {
        assert!(!l.landlocked, "a civic lot of unusable ground at {}", l.center);
    }
    let planned = w.planned_zoning();
    for (i, p) in w.parcels.parcels.iter().enumerate() {
        assert_eq!(planned[i], p.land_use, "parcel at {} planned other than built", p.center);
    }
}

#[test]
fn civic_over_a_whole_block_leaves_no_unusable_ground() {
    let mut w = block();
    let ids = in_block(&w);
    let back = ids.iter().filter(|&&i| !w.parcels.parcels[i].path_frontage).count();
    assert!(back > 0, "the block has backland");
    paint(&mut w, &ids);
    no_unusable_civic(&w);
    assert!(w.lots.values().any(|l| l.land_use == LandUse::Civic), "the fronts are civic");
}

#[test]
fn backland_nothing_could_take_in_is_refused_and_never_a_lot() {
    let mut w = block();
    let ids = in_block(&w);
    let (front, back): (Vec<usize>, Vec<usize>) = ids.iter().partition(|&&i| w.parcels.parcels[i].path_frontage);
    let keys: Vec<_> = back.iter().map(|&i| w.parcels.parcels[i].key).collect();
    paint(&mut w, &front);
    let civic = w.lots.values().filter(|l| l.land_use == LandUse::Civic).count();
    let back: Vec<usize> = keys.iter().filter_map(|k| w.parcels.find(k)).collect();
    // Here the fronts are narrower than the backland behind them: no lot
    // could take it in, and the Zone tool says so.
    for &i in &back {
        assert_eq!(w.parcel_refusal(i, Some(LandUse::Civic)), Some("landlocked: no parcel in front it could be joined to"));
    }
    // Painted anyway (a stroke from before the check): no lot is made of it.
    w.project = None;
    w.add_gesture(Gesture::ParcelZone { parcels: keys.clone(), land_use: Some(LandUse::Civic), industry: None });
    w.try_implement().unwrap();
    no_unusable_civic(&w);
    assert_eq!(w.lots.values().filter(|l| l.land_use == LandUse::Civic).count(), civic, "the front lots stay as they were");
    assert!(!w.master.gestures.values().any(|g| matches!(g, Gesture::ParcelZone { parcels, .. } if parcels.iter().any(|k| keys.contains(k)))), "the backland is taken out of the stroke");
}
