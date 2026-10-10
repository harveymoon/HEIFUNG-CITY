//! [stable-parcels] Road works recut parcels near the roads that changed
//! and nowhere else: lots, buildings, plants and their output elsewhere stay
//! exactly as they were, and the ground under the player's buildings is
//! never reshaped.

use super::*;
use crate::landscape::{LandscapeParams, LandscapePreset};
use crate::power::{BatteryKind, PowerPlantKind};
use crate::services::Placeable;

/// Everything about a lot that the works must leave alone, to the bit.
fn lot_bits(l: &Lot) -> Vec<u32> {
    let mut v: Vec<u32> = l.polygon.iter().flat_map(|p| [p.x.to_bits(), p.y.to_bits()]).collect();
    v.extend([l.center.x, l.center.y, l.front_edge[0].x, l.front_edge[0].y, l.front_edge[1].x, l.front_edge[1].y].map(f32::to_bits));
    v.push(l.land_use as u32);
    v.push(l.landlocked as u32);
    v
}

/// A building's state as the player sees it: where it stands (its lot's
/// outline), how far it got, what it puts out, what ground it stands on.
fn building_bits(w: &World, id: BuildingId) -> (LotId, Vec<u32>, u8, u64, bool, u32, u32, u32) {
    let b = &w.buildings[id];
    let l = &w.lots[b.lot];
    let site = b.plant.map_or(0.0, |k| w.power_site(b.lot, k).unwrap_or(-1.0));
    (b.lot, lot_bits(l), b.floors, b.built_tick, b.construction.is_some(), w.plant_capacity_mw(b).to_bits(), site.to_bits(), w.lot_pad(b.lot).to_bits())
}

/// A grid of streets 140 m apart, housing and shops zoned all over, with
/// growth on many lots and three of the player's plants.
fn grid_city(landscape: LandscapeParams, at: Vec2) -> (World, Vec<BuildingId>) {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape, ..WorldConfig::test_town() });
    w.economy.treasury = 50_000_000.0;
    for i in -3..=3 {
        let c = i as f32 * 140.0;
        w.add_road(&[at + Vec2::new(-420.0, c), at + Vec2::new(420.0, c)], 1, 1);
        w.add_road(&[at + Vec2::new(c, -420.0), at + Vec2::new(c, 420.0)], 1, 1);
    }
    let r = 415.0;
    w.add_zone(vec![at + Vec2::new(-r, -r), at + Vec2::new(r, -r), at + Vec2::new(r, 0.0), at + Vec2::new(-r, 0.0)], LandUse::Residential);
    w.add_zone(vec![at + Vec2::new(-r, 0.0), at + Vec2::new(r, 0.0), at + Vec2::new(r, r), at + Vec2::new(-r, r)], LandUse::Commercial);
    let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none()).map(|(id, _)| id).step_by(3).collect();
    for lot in lots {
        w.build_on(lot);
    }
    // The player's plants, west of the middle: a solar farm, a battery,
    // a coal plant (sized to its site when built).
    let mut plants = Vec::new();
    for (what, p) in [
        (Placeable::Plant(PowerPlantKind::Solar), at + Vec2::new(-210.0, -40.0)),
        (Placeable::Battery(BatteryKind::Small), at + Vec2::new(-210.0, 40.0)),
        (Placeable::Plant(PowerPlantKind::Coal), at + Vec2::new(-350.0, -180.0)),
    ] {
        let lot = w.lots.iter().find(|(_, l)| l.contains(p) && l.usable()).map(|(id, _)| id).or_else(|| w.lots.iter().filter(|(_, l)| l.usable()).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|(id, _)| id)).unwrap();
        let c = w.lots[lot].center;
        let id = w.place_building(what, c).unwrap_or_else(|e| panic!("{what:?} at {c}: {e}"));
        plants.push(id);
    }
    // Let what was placed finish clearing its ground.
    let mut minds = crate::mind::IdleMind;
    for _ in 0..200 {
        w.tick(&mut minds);
    }
    (w, plants)
}

/// The road works the tests do: a street off the east edge of the grid,
/// well away from the plants on the west side.
fn far_road(at: Vec2) -> [Vec2; 2] {
    [at + Vec2::new(420.0, 140.0), at + Vec2::new(560.0, 140.0)]
}

fn check_far_edit_leaves_everything_else(mut w: World, plants: Vec<BuildingId>, at: Vec2) {
    let [a, b] = far_road(at);
    let far = |p: Vec2| crate::geom::project_on_segment(p, a, b).1.distance(p) > 200.0;
    let lots_before: Vec<(LotId, Vec<u32>, Option<BuildingId>)> = w.lots.iter().filter(|(_, l)| far(l.center)).map(|(id, l)| (id, lot_bits(l), l.building)).collect();
    let buildings_before: Vec<(BuildingId, _)> = w.buildings.keys().filter(|&id| far(w.lots[w.buildings[id].lot].center)).map(|id| (id, building_bits(&w, id))).collect();
    let parcels_before: Vec<Vec<u32>> = w.parcels.parcels.iter().filter(|p| far(p.center)).map(|p| p.polygon.iter().flat_map(|v| [v.x.to_bits(), v.y.to_bits()]).collect()).collect();
    assert!(lots_before.len() > 50 && buildings_before.len() > 20, "a town to test on: {} lots, {} buildings", lots_before.len(), buildings_before.len());
    w.add_gesture(Gesture::Road { points: vec![a, b], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
    let check = w.active_project_check();
    assert!(check.refusal.is_none(), "a road far from everything is no trouble: {:?}", check.refusal);
    w.try_implement().expect("implemented");
    let roads_now = w.roads.alive_segments().filter(|(sid, _)| { let (p, q) = w.roads.segment_points(*sid); p.distance(a) < 1.0 || q.distance(b) < 1.0 }).count();
    assert!(roads_now > 0, "the road was laid");
    for (id, bits, building) in &lots_before {
        let l = w.lots.get(*id).unwrap_or_else(|| panic!("lot {id:?} far from the works is still there"));
        assert_eq!(&lot_bits(l), bits, "lot {id:?} at {} is untouched", l.center);
        assert_eq!(&l.building, building, "and has the same building");
    }
    for (id, bits) in &buildings_before {
        assert!(w.buildings.contains_key(*id), "{id:?} still stands");
        assert_eq!(&building_bits(&w, *id), bits, "{} is exactly as it was", w.buildings[*id].name);
    }
    let parcels_after: std::collections::HashSet<Vec<u32>> = w.parcels.parcels.iter().map(|p| p.polygon.iter().flat_map(|v| [v.x.to_bits(), v.y.to_bits()]).collect()).collect();
    for p in &parcels_before {
        assert!(parcels_after.contains(p), "every parcel far from the works is still there, to the bit");
    }
    for id in plants {
        assert!(w.buildings.contains_key(id), "the plant stands");
    }
}

#[test]
fn a_far_road_leaves_lots_buildings_and_plants_byte_identical_on_flat_ground() {
    let (w, plants) = grid_city(LandscapeParams::flat(), Vec2::ZERO);
    check_far_edit_leaves_everything_else(w, plants, Vec2::ZERO);
}

/// A dry square of land on a hilly map for the grid.
fn dry_spot(land: &crate::landscape::Landscape) -> Vec2 {
    for r in 0..12 {
        for k in 0..16 {
            let a = k as f32 / 16.0 * std::f32::consts::TAU;
            let c = Vec2::new(a.cos(), a.sin()) * (r as f32 * 120.0);
            let dry = (-6..=6).all(|i| (-6..=6).all(|j| !land.is_water(c + Vec2::new(i as f32, j as f32) * 100.0) && land.water_distance(c + Vec2::new(i as f32, j as f32) * 100.0) > 30.0));
            if dry {
                return c;
            }
        }
    }
    panic!("no dry square");
}

#[test]
fn a_far_road_leaves_plants_their_ground_and_output_on_hills() {
    let params = LandscapeParams::preset(LandscapePreset::Plains, 3);
    let land = crate::landscape::Landscape::new(&params);
    let at = dry_spot(&land);
    let (w, plants) = grid_city(params, at);
    assert!(!w.landscape.is_flat());
    check_far_edit_leaves_everything_else(w, plants, at);
}

/// The ground under a player's building is locked: a road laid along it
/// (not over it) leaves its outline, its output and its pad as they were,
/// though the parcels next to it are recut.
#[test]
fn a_road_beside_a_plant_does_not_reshape_its_lot() {
    let (mut w, plants) = grid_city(LandscapeParams::flat(), Vec2::ZERO);
    let solar = plants[0];
    let before = building_bits(&w, solar);
    let lot = w.lots[w.buildings[solar].lot].clone();
    // A path-free street just behind the plant's lot, parallel to its front.
    let back = lot.center - lot.front_dir * (lot.depth() * 0.5 + 12.0);
    let along = Vec2::new(-lot.front_dir.y, lot.front_dir.x);
    w.add_road(&[back - along * 60.0, back + along * 60.0], 1, 1);
    assert!(w.buildings.contains_key(solar), "the plant stands");
    assert_eq!(building_bits(&w, solar).1, before.1, "its lot keeps its outline");
    assert_eq!(building_bits(&w, solar).5, before.5, "and its output");
}

/// Parcels are saved with the city: a load gives back exactly the parcels
/// the game had, and road works after the load are as local as before.
#[test]
fn parcels_survive_a_save_and_stay_local_after_it() {
    let (mut w, plants) = grid_city(LandscapeParams::flat(), Vec2::ZERO);
    // Some works first, so the parcels are not what a fresh cut would give.
    w.add_road(&[Vec2::new(-420.0, -70.0), Vec2::new(-280.0, -70.0)], 1, 1);
    let bits = |w: &World| -> Vec<Vec<u32>> { w.parcels.parcels.iter().map(|p| p.polygon.iter().flat_map(|v| [v.x.to_bits(), v.y.to_bits()]).collect()).collect() };
    let before = bits(&w);
    let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
    assert_eq!(bits(&back), before, "the same parcels after a load");
    check_far_edit_leaves_everything_else(back, plants, Vec2::ZERO);
}

/// Nothing far from a change is recut: the cut itself, on its own.
#[test]
fn a_recut_keeps_every_parcel_away_from_the_change() {
    use crate::zoning::{recut_parcels, CutBasis, WaterMask};
    let mut roads = RoadNetwork::default();
    for i in -3..=3 {
        let c = i as f32 * 140.0;
        roads.add_polyline(&[Vec2::new(-420.0, c), Vec2::new(420.0, c)], 1, 1);
        roads.add_polyline(&[Vec2::new(c, -420.0), Vec2::new(c, 420.0)], 1, 1);
    }
    let water = WaterMask::default();
    let clip = Some((Vec2::splat(-800.0), Vec2::splat(800.0)));
    let first = recut_parcels(&[], &CutBasis::default(), &roads, &water, clip, None, &[]);
    assert!(first.parcels.len() > 200);
    let mut more = roads.clone();
    more.add_polyline(&[Vec2::new(420.0, 140.0), Vec2::new(560.0, 140.0)], 1, 1);
    let second = recut_parcels(&first.parcels, &first.basis, &more, &water, clip, None, &[]);
    let near = |p: Vec2| crate::geom::project_on_segment(p, Vec2::new(420.0, 140.0), Vec2::new(560.0, 140.0)).1.distance(p) < 200.0;
    let kept = second.kept.iter().filter(|k| **k).count();
    for p in first.parcels.iter().filter(|p| !near(p.center)) {
        assert!(second.parcels.iter().any(|q| q.polygon == p.polygon), "the parcel at {} is carried over", p.center);
    }
    assert!(kept > first.parcels.len() * 3 / 4, "most parcels are carried over ({kept} of {})", first.parcels.len());
    assert!(second.parcels.len() > first.parcels.len(), "the new street has parcels along it");
    // And the same again is the same: no change, no recut.
    let third = recut_parcels(&second.parcels, &second.basis, &more, &water, clip, None, &[]);
    assert!(third.kept.iter().all(|k| *k));
    assert_eq!(third.parcels.len(), second.parcels.len());
}
