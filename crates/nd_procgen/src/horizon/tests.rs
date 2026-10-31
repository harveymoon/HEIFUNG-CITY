//! [horizon] The far landscape meets the map exactly, is the same for the
//! same seed, carries the coast and sea on, and keeps to its budget.

use super::towns::{place, skyline};
use super::ways::{lane_lights, lane_starts, lanes, way_ends, ways};
use super::*;
use crate::terrain::{build_water, LandscapeTiles};
use nd_sim::landscape::{LandscapeParams, LandscapePreset, WAY_REACH};
use nd_sim::{World, WorldConfig};

fn world(preset: LandscapePreset, seed: u64, climate: Climate) -> World {
    World::new(WorldConfig { landscape: LandscapeParams { climate, ..LandscapeParams::preset(preset, seed) }, ..WorldConfig::default() })
}

fn seam_of(w: &World, pal: &Palette) -> Seam {
    let tiles = LandscapeTiles::new(w, pal, 8.0);
    Seam::from_meshes((0..tiles.len()).map(|t| tiles.mesh(t)), LAND_HALF, 8.0)
}

fn build(w: &World, pal: &Palette) -> FarGround {
    let far = FarLand::new(&w.landscape);
    far_ground(&far, &|p| w.ground_height(p), &seam_of(w, pal), pal)
}

fn hash(meshes: &[MeshData]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for m in meshes {
        for p in m.positions.iter().chain(m.normals.iter()) {
            for v in p {
                v.to_bits().hash(&mut h);
            }
        }
        for c in &m.colors {
            for v in c {
                v.to_bits().hash(&mut h);
            }
        }
        m.indices.hash(&mut h);
    }
    h.finish()
}

/// The far ground's first row of vertices is the playable ground's last,
/// height, normal and colour, all the way round; the next row out is only
/// a hair lower (no step, no gap), and the far land adds nothing at the edge.
#[test]
fn the_far_ground_meets_the_map_exactly() {
    let pal = Palette::default();
    for preset in [LandscapePreset::Coast, LandscapePreset::Islands, LandscapePreset::RiverValley] {
        let w = world(preset, 3, Climate::Temperate);
        let seam = seam_of(&w, &pal);
        assert_eq!(seam.len(), (LAND_HALF * 2.0 / 8.0) as usize * 4, "{preset:?}: the tiles' edge vertices");
        let far = build(&w, &pal);
        let mut on_seam = 0;
        let mut next = 0;
        for m in &far.ground {
            for (i, p) in m.positions.iter().enumerate() {
                let q = Vec2::new(p[0], p[2]);
                let r = q.x.abs().max(q.y.abs());
                if (r - LAND_HALF).abs() < 0.01 {
                    let s = seam.get(q).expect("a seam vertex the tiles have");
                    assert_eq!(p[1].to_bits(), s.h.to_bits(), "{preset:?} height at {q}");
                    assert_eq!(m.normals[i], s.normal);
                    assert_eq!(m.colors[i], s.rgba);
                    on_seam += 1;
                } else if (r - LAND_HALF - 8.0).abs() < 0.01 {
                    let d = w.ground_height(q) - 0.5 - p[1];
                    assert!(d.abs() < 0.6, "{preset:?}: a step of {d} m one cell out at {q}");
                    next += 1;
                }
            }
        }
        // Every seam vertex, the quadrants' shared ones once per quadrant.
        assert!(on_seam >= seam.len(), "{preset:?}: {on_seam} of {}", seam.len());
        assert!(next > 1700, "{preset:?}: {next}");
        let land = FarLand::new(&w.landscape);
        for k in 0..400 {
            let t = k as f32 / 400.0 * 2.0 - 1.0;
            for q in [Vec2::new(LAND_HALF, t * LAND_HALF), Vec2::new(t * LAND_HALF, -LAND_HALF), Vec2::new(t * 900.0, t * 300.0)] {
                assert_eq!(land.extra(q), 0.0, "{q}");
            }
        }
    }
}

/// The same seed makes the same far land, ground, water, towns and
/// ways, bit for bit; another seed another.
#[test]
fn the_far_landscape_is_deterministic_per_seed() {
    let pal = Palette::default();
    let a = world(LandscapePreset::Coast, 11, Climate::Arctic);
    let b = world(LandscapePreset::Coast, 11, Climate::Arctic);
    let c = world(LandscapePreset::Coast, 12, Climate::Arctic);
    let ground = |w: &World| {
        let g = build(w, &pal);
        hash(&g.ground) ^ hash(&g.ridges).rotate_left(7)
    };
    let water = |w: &World| hash(&far_water(&FarLand::new(&w.landscape), &pal));
    let towns = |w: &World| {
        let far = FarLand::new(&w.landscape);
        let exits: Vec<Vec2> = way_ends(w).iter().map(|e| e.dir).collect();
        let t = place(&far, &exits, &|p| far.natural(p));
        let sk: Vec<MeshData> = t.iter().flat_map(|t| { let s = skyline(t, &|o| far.natural(t.pos + o) - t.ground); [s.towers, s.lights, s.glow] }).collect();
        (format!("{t:?}"), hash(&sk))
    };
    assert_eq!(ground(&a), ground(&b));
    assert_eq!(water(&a), water(&b));
    assert_eq!(towns(&a), towns(&b));
    assert_ne!(ground(&a), ground(&c));
    assert_ne!(water(&a), water(&c));
    // Asking in another order changes nothing: the land is a function.
    let far = FarLand::new(&a.landscape);
    let pts: Vec<Vec2> = (0..200).map(|k| Vec2::new((k as f32 * 977.0) % 30000.0 - 15000.0, (k as f32 * 613.0) % 30000.0 - 15000.0)).collect();
    let fwd: Vec<u32> = pts.iter().map(|p| far.natural(*p).to_bits()).collect();
    let back: Vec<u32> = pts.iter().rev().map(|p| FarLand::new(&b.landscape).natural(*p).to_bits()).collect();
    assert_eq!(fwd, back.into_iter().rev().collect::<Vec<_>>());
}

/// A coast runs on east and west past the generated square (no straight
/// edge of sea where the map's sea polygon ended), never north towards
/// the railway, and the sea stays open out to the horizon.
#[test]
fn the_coast_and_sea_run_on_past_the_map() {
    for seed in [1, 5, 9] {
        let w = world(LandscapePreset::Coast, seed, Climate::Desert);
        let far = FarLand::new(&w.landscape);
        let sea = w.landscape.sea.as_ref().unwrap();
        let north = sea.coast.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
        // The map's sea polygon stopped at x = ±2100: past it, the sea goes on.
        for x in [-9000.0, -4000.0, -2400.0, 2400.0, 4000.0, 9000.0] {
            let mut wet_below = false;
            for y in [-4000.0, -8000.0, -15000.0] {
                wet_below |= far.is_water(Vec2::new(x, y));
            }
            assert!(wet_below, "seed {seed}: no sea south at x {x}");
        }
        // The sea never comes north of the coast's northernmost point.
        for k in 0..400 {
            let x = -16000.0 + k as f32 * 80.0;
            for y in [north + 170.0, nd_sim::rail::TRUNK_Y] {
                let p = Vec2::new(x, y);
                assert!(!far.is_sea(p), "seed {seed}: sea north of the coast at {p}");
            }
        }
        // Open sea out to the horizon (a far island or two in it).
        let wet = (0..200).filter(|k| far.is_water(Vec2::new(-15000.0 + *k as f32 * 150.0, -HORIZON + 300.0))).count();
        assert!(wet > 110, "seed {seed}: {wet} of 200 wet along the horizon");
        // The fine sheet's edge and the far water's first ring meet: no
        // gap along the sea south of the map.
        let pal = Palette::default();
        let fine = build_water(&w, &pal);
        let rings = far_water(&far, &pal);
        for x in [-1500.0, 0.0, 1500.0] {
            assert!(covered(&fine, Vec2::new(x, -FINE_WATER + 5.0)), "fine sheet at {x}");
            assert!(rings.iter().any(|m| covered(m, Vec2::new(x, -FINE_WATER - 5.0))), "far water at {x}");
        }
        // East of the old edge, where the map's sea had ended in a straight line.
        let p = Vec2::new(FINE_WATER + 300.0, -3000.0);
        if far.is_water(p) {
            assert!(rings.iter().any(|m| covered(m, p)));
        }
    }
}

/// Islands far out to sea only past where ships leave the map.
#[test]
fn far_islands_stand_past_the_ships_way_out() {
    let w = world(LandscapePreset::Islands, 4, Climate::Tropical);
    let far = FarLand::new(&w.landscape);
    let mut land_far = 0;
    for k in 0..720 {
        let a = k as f32 / 720.0 * std::f32::consts::TAU;
        let dir = Vec2::new(a.cos(), a.sin());
        for r in [WAY_REACH - 200.0, WAY_REACH + 400.0] {
            assert!(far.is_water(dir * r), "land at {} on a ship's way out", dir * r);
        }
        land_far += (!far.is_water(dir * 12000.0)) as usize;
    }
    assert!(land_far > 20 && land_far < 600, "{land_far}");
}

fn covered(m: &MeshData, p: Vec2) -> bool {
    m.indices.chunks(3).any(|t| {
        let v = |i: u32| Vec2::new(m.positions[i as usize][0], m.positions[i as usize][2]);
        let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
        let s = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
        let (d1, d2, d3) = (s(a, b, p), s(b, c, p), s(c, a, p));
        let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
        let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
        !(neg && pos)
    })
}

/// The whole horizon keeps to its triangle and draw budget: the far
/// ground under 150k triangles in four meshes (the old single mesh was
/// 320k out to 5.3 km; this reaches 12 km, and the ridges 20), the ridges
/// and water small, each town a few thousand.
#[test]
fn the_horizon_keeps_to_its_budget() {
    let pal = Palette::default();
    for (preset, climate) in [(LandscapePreset::Coast, Climate::Arctic), (LandscapePreset::Islands, Climate::Tropical), (LandscapePreset::Delta, Climate::Temperate), (LandscapePreset::RiverValley, Climate::Volcanic), (LandscapePreset::Plains, Climate::Desert)] {
        let w = world(preset, 6, climate);
        let t0 = std::time::Instant::now();
        let g = build(&w, &pal);
        let built = t0.elapsed();
        let ground: usize = g.ground.iter().map(|m| m.indices.len() / 3).sum();
        let ridges: usize = g.ridges.iter().map(|m| m.indices.len() / 3).sum();
        assert!(ground < 150_000, "{preset:?}: far ground {ground} triangles");
        // Every climate's colours and shapes are numbers (a desert has no
        // snow line: no NaN from it).
        for m in g.ground.iter().chain(g.ridges.iter()) {
            assert!(m.positions.iter().chain(m.normals.iter()).flatten().chain(m.colors.iter().flatten()).all(|v| v.is_finite()), "{preset:?} {climate:?}: a NaN");
        }
        assert!(ridges < 40_000, "{preset:?}: ridges {ridges}");
        let far = FarLand::new(&w.landscape);
        let water: usize = far_water(&far, &pal).iter().map(|m| m.indices.len() / 3).sum();
        assert!(water < 90_000, "{preset:?}: far water {water}");
        let ends = way_ends(&w);
        assert!(ends.len() >= 4, "{preset:?}: the highway and trunk both ways: {}", ends.len());
        let exits: Vec<Vec2> = ends.iter().map(|e| e.dir).collect();
        let height = |p: Vec2| far.natural(p);
        let towns = place(&far, &exits, &height);
        assert!((2..=9).contains(&towns.len()), "{preset:?}: {} towns", towns.len());
        let mut town_tris = 0;
        for t in &towns {
            let s = skyline(t, &|o| far.natural(t.pos + o) - t.ground);
            assert!(s.triangles() < 9_000, "{preset:?}: a town of {} triangles", s.triangles());
            town_tris += s.triangles();
        }
        let (ribbons, lamps) = ways(&ends, &towns, &height);
        let lane_mesh = lane_lights(&lanes(&far, &lane_starts(&w)));
        let rest = (ribbons.indices.len() + lamps.indices.len() + lane_mesh.indices.len()) / 3;
        assert!(town_tris + rest < 60_000, "{preset:?}: towns {town_tris}, ways and lanes {rest}");
        eprintln!("{preset:?}: ground {ground}, ridges {ridges}, water {water}, towns {} ({town_tris}), ways+lanes {rest}, built in {built:?}", towns.len());
    }
}

/// Towns stand on dry, fairly level far land past the map, apart, and
/// one where the highway leads each way.
#[test]
fn towns_stand_where_the_ways_lead() {
    for preset in [LandscapePreset::Coast, LandscapePreset::RiverValley, LandscapePreset::Islands] {
        let w = world(preset, 2, Climate::Temperate);
        let far = FarLand::new(&w.landscape);
        let ends = way_ends(&w);
        let exits: Vec<Vec2> = ends.iter().map(|e| e.dir).collect();
        let towns = place(&far, &exits, &|p| far.natural(p));
        for t in &towns {
            assert!(!far.is_water(t.pos), "{preset:?}: a town in the sea at {}", t.pos);
            assert!(t.pos.length() >= towns::NEAREST - 1.0, "{preset:?}: {}", t.pos);
            for u in &towns {
                assert!(t == u || t.pos.distance(u.pos) > 2000.0);
            }
        }
        if preset != LandscapePreset::Islands {
            // The highway leaves east and west.
            assert!(towns.iter().any(|t| t.on_way && t.pos.x > 3000.0), "{preset:?}: no town east: {towns:?}");
            assert!(towns.iter().any(|t| t.on_way && t.pos.x < -3000.0), "{preset:?}: no town west: {towns:?}");
        }
    }
}

/// [r4-edge-bridges] Harvey's flag "weird straight borders on the water":
/// past the map the far ground's coarse cells drew the shore as a
/// staircase along the grid. Where the far ground crosses the waterline
/// now it is on the shore: every crossing on a cell side lies within a few
/// metres of where the far land puts the water's edge.
#[test]
fn the_far_waterline_follows_the_shore() {
    let pal = Palette::default();
    for (preset, seed) in [(LandscapePreset::RiverValley, 7), (LandscapePreset::Delta, 3), (LandscapePreset::Coast, 3), (LandscapePreset::Islands, 3)] {
        // (Not the plains: their streams are narrower than these cells.)
        let w = world(preset, seed, Climate::Temperate);
        let far = FarLand::new(&w.landscape);
        let g = build(&w, &pal);
        let (mut n, mut off, mut worst) = (0usize, 0usize, 0.0f32);
        for m in &g.ground {
            for t in m.indices.chunks(3) {
                for (i, j) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                    let (a, b) = (m.positions[i as usize], m.positions[j as usize]);
                    let (pa, pb) = (Vec2::new(a[0], a[2]), Vec2::new(b[0], b[2]));
                    let r = pa.x.abs().max(pa.y.abs()).min(pb.x.abs().max(pb.y.abs()));
                    // Past the first rings, out to where the cells (64 m and
                    // more) are wider than a river and the fog thickens.
                    if r < LAND_HALF + 48.0 || r > 3200.0 || (a[1] - WATER_LEVEL).signum() == (b[1] - WATER_LEVEL).signum() {
                        continue;
                    }
                    let q = pa.lerp(pb, (WATER_LEVEL - a[1]) / (b[1] - a[1]));
                    let d = far.water_distance(q).abs();
                    n += 1;
                    off += (d > 6.0) as usize;
                    worst = worst.max(d);
                }
            }
        }
        if n == 0 {
            continue;
        }
        // (A few where a stream not much wider than a cell runs.)
        assert!(off * 20 <= n, "{preset:?}: {off} of {n} waterline crossings more than 6 m off the shore (worst {worst:.1} m)");
    }
}

/// [r4-edge-bridges] Past the map only the sea freezes, as on the map: an
/// Arctic river ran open to the edge and froze over in a straight line
/// 400 m past it, where the fine sheet gave way to the far water.
#[test]
fn an_arctic_river_runs_open_past_the_map() {
    let pal = Palette::default();
    let ice = |c: &[f32; 4]| (c[3] - 0.97).abs() < 1e-3;
    let w = world(LandscapePreset::RiverValley, 7, Climate::Arctic);
    let far = FarLand::new(&w.landscape);
    let fine = build_water(&w, &pal);
    let rings = far_water(&far, &pal);
    let outside = |p: &[f32; 3]| p[0].abs().max(p[2].abs()) > LAND_HALF;
    let frozen = fine.positions.iter().zip(&fine.colors).filter(|(p, c)| outside(p) && ice(c)).count() + rings.iter().map(|m| m.colors.iter().filter(|c| ice(c)).count()).sum::<usize>();
    assert_eq!(frozen, 0, "no ice on a river past the map");
    // A coast's sea still freezes along its shore, on the map and past it.
    let w = world(LandscapePreset::Coast, 7, Climate::Arctic);
    let far = FarLand::new(&w.landscape);
    let fine = build_water(&w, &pal);
    let rings = far_water(&far, &pal);
    let inside_ice = fine.positions.iter().zip(&fine.colors).filter(|(p, c)| !outside(p) && ice(c)).count();
    let past_ice = fine.positions.iter().zip(&fine.colors).filter(|(p, c)| outside(p) && ice(c)).count() + rings.iter().map(|m| m.colors.iter().filter(|c| ice(c)).count()).sum::<usize>();
    assert!(inside_ice > 100 && past_ice > 100, "sea ice on the map {inside_ice} and past it {past_ice}");
}
