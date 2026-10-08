//! [island-lots] Lots on hillsides and on islands off the coast: ordinary
//! slopes are graded (terraced lot by lot), only cliffs are refused, with a
//! plain reason; streets past the configured square still get parcels.

use super::*;
use crate::landscape::{LandscapeParams, LandscapePreset, MAX_GRADED_SLOPE};

fn hilly_world(preset: LandscapePreset) -> World {
    World::new(WorldConfig { landscape: LandscapeParams::preset(preset, 7), ..WorldConfig::test_town() })
}

/// A dry spot `r` metres clear of any water (the distance is capped at
/// about 160 m), between `from` and `to` out from the centre (on the
/// farther axis); the first found scanning rows from the south-west.
fn dry_spot(w: &World, from: f32, to: f32, r: f32) -> Vec2 {
    let mut y = -to;
    while y <= to {
        let mut x = -to;
        while x <= to {
            let p = Vec2::new(x, y);
            let c = x.abs().max(y.abs());
            if c >= from && w.landscape.water_distance(p) > r {
                return p;
            }
            x += 40.0;
        }
        y += 40.0;
    }
    panic!("no dry spot between {from} and {to}");
}

/// The natural ground round `at` replaced by a plane rising `slope` along
/// +y (out to `reach` metres, then level), so a street along x runs on the
/// contour with the hill rising behind one side and falling from the other.
fn hillside(w: &mut World, at: Vec2, slope: f32, reach: f32) {
    w.landscape.reshape_for_test(move |p, h| {
        let d = p - at;
        if d.x.abs() > 400.0 || d.y.abs() > 400.0 {
            return h;
        }
        30.0 + slope * d.y.clamp(-reach, reach)
    });
}

fn street_parcels(w: &World, at: Vec2) -> Vec<usize> {
    w.parcels.in_circle(at, 70.0).into_iter().filter(|&i| w.parcels.parcels[i].street_frontage).collect()
}

#[test]
fn a_street_on_a_hillside_gets_graded_lots_that_can_be_zoned() {
    for slope in [0.3, 0.45] {
        let mut w = hilly_world(LandscapePreset::Plains);
        let at = dry_spot(&w, 200.0, 700.0, 150.0);
        hillside(&mut w, at, slope, 400.0);
        w.add_road(&[at - Vec2::new(120.0, 0.0), at + Vec2::new(120.0, 0.0)], 1, 1);
        let ids = street_parcels(&w, at);
        assert!(ids.len() >= 4, "slope {slope}: parcels along the street ({})", ids.len());
        let ok: Vec<usize> = ids.iter().copied().filter(|&i| w.parcel_refusal(i, Some(LandUse::Residential)).is_none()).collect();
        for &i in &ids {
            if let Some(why) = w.parcel_refusal(i, Some(LandUse::Residential)) {
                println!("slope {slope}: parcel {i} refused: {why}");
            }
        }
        assert!(ok.len() * 4 >= ids.len() * 3, "slope {slope}: most street parcels take homes ({}/{})", ok.len(), ids.len());
        let (_, n) = w.paint_parcels(&ok, Some(LandUse::Residential), None);
        assert!(n >= 4);
        w.implement();
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.usable()).map(|(id, _)| id).collect();
        assert!(lots.len() >= 4, "slope {slope}: usable lots ({})", lots.len());
        // Graded: each lot's ground is its pad, level with its street.
        for id in lots {
            let l = &w.lots[id];
            let pad = w.ground.pads[&id];
            let g = w.ground_height(l.center);
            assert!((g - pad).abs() < 0.3, "slope {slope}: lot ground {g:.2} at its pad {pad:.2}");
        }
    }
}

#[test]
fn a_cliff_refuses_with_a_plain_reason() {
    let mut w = hilly_world(LandscapePreset::Plains);
    let at = dry_spot(&w, 200.0, 700.0, 150.0);
    hillside(&mut w, at, MAX_GRADED_SLOPE * 1.6, 400.0);
    w.add_road(&[at - Vec2::new(120.0, 0.0), at + Vec2::new(120.0, 0.0)], 1, 1);
    let ids = street_parcels(&w, at);
    assert!(!ids.is_empty(), "the parcels are cut, only refused");
    for &i in &ids {
        let why = w.parcel_refusal(i, Some(LandUse::Residential)).expect("a cliff is refused");
        assert!(why.contains("cliff"), "says why: {why}");
        // Parks still go anywhere.
        assert!(w.parcel_refusal(i, Some(LandUse::Park)).is_none());
    }
}

#[test]
fn a_street_on_an_island_past_the_configured_square_gets_parcels() {
    let mut w = hilly_world(LandscapePreset::Islands);
    let h = w.config.half_extent;
    let at = dry_spot(&w, h + 150.0, crate::landscape::LAND_HALF - 300.0, 90.0);
    // Level enough to build on, and not joined to anything.
    w.landscape.reshape_for_test(move |p, z| if p.distance(at) < 150.0 { 12.0 } else { z });
    let before = w.parcels.len();
    w.add_road(&[at - Vec2::new(60.0, 0.0), at + Vec2::new(60.0, 0.0)], 1, 1);
    assert!(w.parcel_half() > h, "the parcels' square takes the street in");
    let ids = street_parcels(&w, at);
    assert!(w.parcels.len() > before && ids.len() >= 2, "parcels along the island street ({})", ids.len());
    assert!(ids.iter().any(|&i| w.parcel_refusal(i, Some(LandUse::Residential)).is_none()), "and they take homes");
    // The ground raster reaches it, so the lots' pads are levelled.
    assert!(w.ground_half() >= at.x.abs().max(at.y.abs()) + 60.0);
    // Off the parcels, the Zone tool is told why.
    assert!(w.no_parcel_why(at + Vec2::new(0.0, 300.0)).is_some());
    assert!(w.no_parcel_why(w.parcels.parcels[ids[0]].center).is_none());
}

#[test]
fn the_parcels_square_stays_put_without_far_streets() {
    let w = hilly_world(LandscapePreset::Plains);
    assert_eq!(w.parcel_half(), w.config.half_extent);
    assert_eq!(w.ground_half(), w.config.half_extent + 200.0);
}

#[test]
fn a_saved_city_on_a_hill_keeps_its_lots() {
    let mut w = hilly_world(LandscapePreset::Plains);
    let at = dry_spot(&w, 200.0, 700.0, 150.0);
    w.add_road(&[at - Vec2::new(120.0, 0.0), at + Vec2::new(120.0, 0.0)], 1, 1);
    let ids = street_parcels(&w, at);
    w.paint_parcels(&ids, Some(LandUse::Residential), None);
    w.implement();
    let lots: Vec<(Vec2, bool)> = w.lots.values().map(|l| (l.center, l.landlocked)).collect();
    assert!(!lots.is_empty());
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    let again: Vec<(Vec2, bool)> = back.lots.values().map(|l| (l.center, l.landlocked)).collect();
    assert_eq!(lots, again, "the same lots, as usable as they were");
    assert!(back.parcels.parcels.iter().filter(|p| p.lot.is_some()).count() >= ids.len().min(1));
}
