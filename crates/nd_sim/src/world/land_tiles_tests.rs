//! [r4-tiles] Buying land round the city: the bought ground is the far
//! land the horizon showed, the starting map is untouched, roads and trips
//! run across the seam, saves keep only which tiles are owned.

use super::*;
use crate::landscape::far::FarLand;
use crate::landscape::{outer_tiles, tile_of, tile_rect, LandscapeParams, LandscapePreset, GROWN_HALF, LAND_HALF};

fn world(preset: LandscapePreset, seed: u64) -> World {
    World::new(WorldConfig { emergencies: false, landscape: LandscapeParams::preset(preset, seed), ..WorldConfig::default() })
}

/// Points on the raster's 10 m grid in the ring of tiles (where heights are
/// stored exactly, not interpolated), every `step` metres.
fn ring_points(step: f32) -> Vec<Vec2> {
    let mut out = Vec::new();
    let mut y = -GROWN_HALF;
    while y <= GROWN_HALF {
        let mut x = -GROWN_HALF;
        while x <= GROWN_HALF {
            let p = Vec2::new(x, y);
            // (Not the outermost row: the raster's edge reads a hair in.)
            if p.x.abs().max(p.y.abs()) > LAND_HALF && p.x.abs().max(p.y.abs()) < GROWN_HALF - 1.0 {
                out.push(p);
            }
            x += step;
        }
        y += step;
    }
    out
}

#[test]
fn bought_ground_is_the_far_land_the_horizon_showed_and_the_start_map_is_untouched() {
    for (preset, seed) in [(LandscapePreset::Coast, 7), (LandscapePreset::RiverValley, 11), (LandscapePreset::Delta, 3), (LandscapePreset::Islands, 5), (LandscapePreset::Plains, 9)] {
        let w = world(preset, seed);
        let before: &Landscape = &w.landscape;
        let grown = before.with_tiles(&[(2, 0)]);
        assert!(grown.is_grown() && grown.land_half() == GROWN_HALF);
        let far = FarLand::new(before);
        // Past the map: the far ground as drawn (the map's ground plus the
        // far land's `extra`) is the bought ground, to the millimetre.
        let mut worst = 0.0f32;
        for p in ring_points(50.0) {
            let seen = before.height(p) + far.extra(p);
            let got = grown.height(p);
            worst = worst.max((seen - got).abs());
        }
        assert!(worst < 1e-3, "{preset:?}: bought ground off the horizon's by {worst} m");
        // The water: where the far landscape drew water, bought land has it.
        let pts = ring_points(30.0);
        let wrong = pts.iter().filter(|p| far.water_distance(**p).abs() > 2.0 && (far.water_distance(**p) < 0.0) != grown.is_water(**p)).count();
        assert!(wrong * 1000 <= pts.len(), "{preset:?}: {wrong} of {} points wet on one and dry on the other", pts.len());
        // The starting map: the same heights and water, every 10 m cell
        // (short of its last row, which the old raster read a hair in from).
        let mut y = -LAND_HALF;
        while y < LAND_HALF {
            let mut x = -LAND_HALF;
            while x < LAND_HALF {
                let p = Vec2::new(x, y);
                assert_eq!(before.height(p).to_bits(), grown.height(p).to_bits(), "{preset:?}: height moved at {p:?}");
                assert_eq!(before.water_distance(p).to_bits(), grown.water_distance(p).to_bits(), "{preset:?}: water moved at {p:?}");
                assert_eq!(before.is_water(p), grown.is_water(p), "{preset:?}: water at {p:?}");
                x += 10.0;
            }
            y += 10.0;
        }
        // And across the seam the ground runs on: no step between the last
        // cell of the map and the first of the tile.
        for k in -17..=17 {
            let y = k as f32 * 100.0;
            let (a, b) = (grown.height(Vec2::new(LAND_HALF - 10.0, y)), grown.height(Vec2::new(LAND_HALF + 10.0, y)));
            assert!((a - b).abs() < 8.0, "{preset:?}: a {:.1} m step across the seam at y {y}", (a - b).abs());
        }
    }
}

#[test]
fn a_coast_runs_on_into_bought_land_and_its_sea_is_the_sea() {
    let w = world(LandscapePreset::Coast, 7);
    let grown = w.landscape.with_tiles(&[(2, -1)]);
    let far = FarLand::new(&w.landscape);
    // Down the east tile at x = 2400: sea in the south, land in the north,
    // the shore where the far land had it.
    let wet: Vec<bool> = (0..60).map(|k| grown.is_water(Vec2::new(2400.0, -2900.0 + k as f32 * 40.0))).collect();
    assert!(wet[0] && !wet[59], "sea to the south, land to the north: {wet:?}");
    let hit = grown.water_at(Vec2::new(2400.0, -2900.0)).expect("the sea");
    assert_eq!(hit.kind, crate::landscape::WaterKind::Sea);
    assert!(far.is_sea(Vec2::new(2400.0, -2900.0)));
    assert!(grown.nearest_shore(Vec2::new(2400.0, 0.0), 3000.0).is_some_and(|s| s.water.kind == crate::landscape::WaterKind::Sea));
}

#[test]
fn buying_a_tile_takes_the_money_and_opens_the_next_ones() {
    let mut w = world(LandscapePreset::RiverValley, 11);
    let tiles = w.land_tiles();
    assert_eq!(tiles.len(), 16);
    assert_eq!(tiles.iter().filter(|t| t.buyable).count(), 12, "the twelve beside the map, not the corners");
    let east = tiles.iter().find(|t| t.tile() == (2, 0)).unwrap().clone();
    assert!(east.highway, "the highway runs out east: {}", east.summary());
    assert!(east.price >= 5_000.0 && !east.summary().is_empty());
    assert!(w.tile_offer((2, 2)).unwrap_err().contains("beside"), "a corner first needs a side");
    assert!(w.tile_offer((3, 0)).is_err() && w.tile_offer((1, 0)).is_err());
    w.economy.treasury = east.price - 1.0;
    assert!(w.buy_tile((2, 0)).unwrap_err().contains("treasury"));
    w.economy.treasury = 1_000_000.0;
    let paid = w.buy_tile((2, 0)).expect("bought");
    assert_eq!(paid, east.price);
    assert!((w.economy.treasury - (1_000_000.0 - paid)).abs() < 1.0);
    assert!(w.landscape.owns(Vec2::new(2500.0, 0.0)) && w.landscape.owns_tile((2, 0)));
    assert!(w.tile_offer((2, 0)).unwrap_err().contains("already"));
    // The corners beside it open; each later tile costs more.
    let after = w.land_tiles();
    assert!(after.iter().any(|t| t.tile() == (2, 1) && t.buyable));
    let north = after.iter().find(|t| t.tile() == (0, 2)).unwrap();
    let north_before = tiles.iter().find(|t| t.tile() == (0, 2)).unwrap();
    assert!(north.price > north_before.price, "{} then {}", north_before.price, north.price);
    // The grids reach over the new land.
    assert!(w.land_value.half >= GROWN_HALF - 1.0 && w.estate.pressure.half >= GROWN_HALF - 1.0);
    assert!(w.contamination.half >= GROWN_HALF);
}

#[test]
fn bought_tiles_are_saved_and_loaded_and_an_old_save_loads_as_the_start_map() {
    let mut w = world(LandscapePreset::Coast, 7);
    let old = w.to_ron().unwrap();
    assert!(!old.contains("tiles"), "a save with no land bought is written as before");
    let back = World::from_ron(&old).unwrap();
    assert!(back.landscape.tiles.is_empty() && back.landscape.land_half() == LAND_HALF);
    w.economy.treasury = 1_000_000.0;
    w.buy_tile((0, 2)).unwrap();
    w.buy_tile((1, 2)).unwrap();
    let text = w.to_ron().unwrap();
    let back = World::from_ron(&text).unwrap();
    assert_eq!(back.landscape.tiles, vec![(0, 2), (1, 2)]);
    for p in [Vec2::new(100.0, 2600.0), Vec2::new(1300.0, 2900.0), Vec2::new(-2600.0, 0.0)] {
        assert_eq!(back.landscape.height(p).to_bits(), w.landscape.height(p).to_bits(), "{p:?}");
    }
    // Only the tile list is saved, not its ground: the landscape as saved
    // grew by a few bytes (the city's grids grew with the land).
    let land_old = ron::to_string(&*World::from_ron(&old).unwrap().landscape).unwrap();
    let land_new = ron::to_string(&*w.landscape).unwrap();
    assert!(land_new.len() < land_old.len() + 100, "{} then {}", land_old.len(), land_new.len());
    assert!(back.landscape.owns(Vec2::new(1300.0, 2900.0)) && !back.landscape.owns(Vec2::new(-1300.0, 2900.0)));
}

/// A dry row of land for a road from the map into the east tile.
fn dry_row(w: &World) -> f32 {
    let land = w.landscape.with_tiles(&[(2, 0)]);
    (0..20)
        .map(|k| -300.0 + k as f32 * 30.0)
        .filter(|y| (*y - crate::landscape::HIGHWAY_Y).abs() > 80.0 && (*y - crate::rail::TRUNK_Y).abs() > 80.0)
        .find(|&y| (0..=60).all(|i| !land.is_water(Vec2::new(1300.0 + i as f32 * 25.0, y)) && !land.is_water(Vec2::new(1300.0 + i as f32 * 25.0, y + 60.0))))
        .expect("a dry row")
}

#[test]
fn a_road_and_a_trip_run_across_the_seam() {
    let mut w = world(LandscapePreset::Plains, 9);
    let y = dry_row(&w);
    w.economy.treasury = 1_000_000.0;
    w.buy_tile((2, 0)).unwrap();
    // Straight across the seam, ending inside the bought tile: an ordinary
    // road, not a way to elsewhere.
    w.add_road(&[Vec2::new(1300.0, y), Vec2::new(2800.0, y)], 1, 1);
    let ends: Vec<Vec2> = w.roads.leaf_nodes().into_iter().map(|n| w.roads.node(n).pos).collect();
    assert!(ends.iter().any(|p| p.distance(Vec2::new(2800.0, y)) < 1.0), "the road ends in the tile: {ends:?}");
    // Parcels on both sides of the seam; zoning makes lots in the tile.
    assert!(w.parcels.parcels.iter().any(|p| p.center.x > LAND_HALF + 50.0), "parcels on the bought land");
    w.add_zone(vec![Vec2::new(1350.0, y + 5.0), Vec2::new(2750.0, y + 5.0), Vec2::new(2750.0, y + 60.0), Vec2::new(1350.0, y + 60.0)], LandUse::Residential);
    assert!(w.lots.values().any(|l| l.center.x > LAND_HALF + 50.0), "lots on the bought land");
    assert!(w.lots.values().any(|l| l.center.x < LAND_HALF - 50.0), "and on the map");
    // A drive and a walk from the map to the tile.
    let near = |w: &World, x: f32| w.roads.nearest_where(Vec2::new(x, y), |s| s.is_street()).unwrap();
    let (a, _, pa) = near(&w, 1400.0);
    let (b, _, pb) = near(&w, 2700.0);
    let (lanes, _, _) = w.traffic.net.find_route(a, pa, b, pb).expect("a drive across the seam");
    assert!(!lanes.is_empty(), "{lanes:?}");
    assert!(w.walk.route(pa, pb).is_some(), "a walk across the seam");
    // The ground is graded under it on both sides.
    let z = |x: f32| w.ground.height(&w.landscape, Vec2::new(x, y));
    assert!(z(LAND_HALF - 5.0).is_finite() && (z(LAND_HALF - 5.0) - z(LAND_HALF + 5.0)).abs() < 2.0);
    // A road drawn to the tile's far edge leads on elsewhere.
    w.add_road(&[Vec2::new(2700.0, y), Vec2::new(GROWN_HALF - 20.0, y)], 1, 1);
    assert!(w.roads.leaf_nodes().into_iter().any(|n| w.roads.node(n).pos.x >= crate::landscape::WAY_REACH - 1.0 && (w.roads.node(n).pos.y - y).abs() < 1.0), "on out to elsewhere");
    // And people get there: tick a while with homes in the tile.
    let tile_lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.center.x > LAND_HALF + 50.0).map(|(id, _)| id).take(4).collect();
    assert!(!tile_lots.is_empty());
    for lot in tile_lots {
        w.build_on(lot);
        if let Some(b) = w.lots[lot].building {
            w.finish_construction(b);
        }
    }
    assert!(w.link_to_highway(Vec2::new(1300.0, y)));
    let mut minds = crate::mind::IdleMind;
    for _ in 0..1500 {
        w.tick(&mut minds);
        if w.citizens.values().any(|c| c.home.and_then(|b| w.building_entrance(b)).is_some_and(|p| p.x > LAND_HALF)) {
            break;
        }
    }
    assert!(w.citizens.values().any(|c| c.home.and_then(|b| w.building_entrance(b)).is_some_and(|p| p.x > LAND_HALF)), "people moved into homes on the bought land");
}

#[test]
fn land_not_bought_takes_no_parcels_and_says_why() {
    let mut w = world(LandscapePreset::Plains, 9);
    let y = dry_row(&w);
    w.economy.treasury = 1_000_000.0;
    w.buy_tile((0, 2)).unwrap();
    // The ring is generated now, but the east tile is not the city's.
    w.add_road(&[Vec2::new(1300.0, y), Vec2::new(2800.0, y)], 1, 1);
    assert!(!w.parcels.parcels.iter().any(|p| p.center.x > LAND_HALF + 10.0), "no parcels on land not bought");
    let why = w.no_parcel_why(Vec2::new(2500.0, y + 20.0)).unwrap_or("");
    assert!(why.contains("not bought"), "{why}");
}

#[test]
fn positions_at_the_far_edge_stay_precise() {
    // The farthest a position on the city's land can be, and past it where
    // ways run on: an f32 there resolves well under a millimetre.
    for v in [GROWN_HALF, crate::landscape::WAY_REACH] {
        let next = f32::from_bits(v.to_bits() + 1);
        assert!(next - v < 0.001, "{v}: {}", next - v);
    }
    // A short road at the far corner keeps its length and its lots.
    let mut w = world(LandscapePreset::Plains, 9);
    w.economy.treasury = 10_000_000.0;
    for t in [(2, 0), (2, 1), (2, 2)] {
        w.buy_tile(t).unwrap();
    }
    let land = &w.landscape;
    let corner = Vec2::splat(GROWN_HALF - 160.0);
    assert!(land.owns(corner) && tile_of(corner) == (2, 2));
    let a = corner - Vec2::new(300.0, 0.0);
    let b = corner;
    let gid = w.add_road(&[a, b], 1, 1);
    let _ = gid;
    let sid = w.roads.nearest_where(corner - Vec2::new(150.0, 0.0), |s| s.is_street()).unwrap().0;
    let (p, q) = w.roads.segment_points(sid);
    assert!((p.distance(q) - (q - p).length()).abs() < 1e-3);
    let (lo, hi) = tile_rect((2, 2));
    assert!(p.x >= lo.x - 1.0 && q.x <= hi.x + 1.0);
    assert!(w.parcels.parcels.iter().any(|x| x.center.distance(corner) < 400.0), "parcels at the far corner");
    // Every tile of the ring is surveyed and none is the flat map's.
    assert_eq!(w.tile_grounds().len(), outer_tiles().len());
}
