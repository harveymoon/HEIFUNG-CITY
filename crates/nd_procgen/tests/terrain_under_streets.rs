//! The terrain never covers a street on the ground. A user flag (day 27,
//! `flag_patch`): a six-lane street down a slope lay under a sheet of pale
//! earth with its lane arrows poking through and the cars at its kerb on
//! the dirt. Three things put the earth there, each tested here on a wide
//! street drawn over a crest with lots zoned along both sides:
//! - the carriageway was drawn straight from node to node while the cars,
//!   the lots and the built ground followed the street's inner height
//!   samples (`World::road_inner`), up to metres higher over a rise;
//! - the terrain mesh is coarser than the built ground, so a vertex on a
//!   cutting's side slope or a lot's pad a few metres off the kerb drew its
//!   triangles over the lanes;
//! - a lot's margin was levelled to its pad over the street's own corridor.
use glam::Vec2;
use nd_procgen::street::{generate_street, StreetInput, StreetLayer};
use nd_procgen::{Palette, Surface};
use nd_sim::landscape::{LandscapeParams, LAND_HALF};
use nd_sim::roads::{project_on_segment, RoadKind};
use nd_sim::{Gesture, LandUse, World, WorldConfig};

/// The cell the renderer draws the terrain at (`render.rs`).
const TERRAIN_CELL: f32 = 8.0;

/// Height of the terrain mesh at `p`: its triangles as built (`a c b`,
/// `b c d` per cell).
fn terrain_mesh_height(pos: &[[f32; 3]], p: Vec2) -> f32 {
    let n = ((LAND_HALF * 2.0) / TERRAIN_CELL).ceil() as usize + 1;
    let fx = (p.x + LAND_HALF) / TERRAIN_CELL;
    let fy = (p.y + LAND_HALF) / TERRAIN_CELL;
    let (i, j) = (fx.floor() as usize, fy.floor() as usize);
    let (u, v) = (fx - i as f32, fy - j as f32);
    let h = |i: usize, j: usize| pos[j * n + i][1];
    let (a, b, c, d) = (h(i, j), h(i + 1, j), h(i, j + 1), h(i + 1, j + 1));
    if u + v <= 1.0 {
        a + (b - a) * u + (c - a) * v
    } else {
        d + (c - d) * (1.0 - u) + (b - d) * (1.0 - v)
    }
}

/// Height of the highest triangle of `tris` over `p`, if any.
fn mesh_height_at(tris: &[[[f32; 3]; 3]], p: Vec2) -> Option<f32> {
    let mut best: Option<f32> = None;
    for t in tris {
        let (a, b, c) = (Vec2::new(t[0][0], t[0][2]), Vec2::new(t[1][0], t[1][2]), Vec2::new(t[2][0], t[2][2]));
        let den = (b - a).perp_dot(c - a);
        if den.abs() < 1e-6 {
            continue;
        }
        let s = (p - a).perp_dot(c - a) / den;
        let r = (b - a).perp_dot(p - a) / den;
        if s < -1e-4 || r < -1e-4 || s + r > 1.0 + 1e-4 {
            continue;
        }
        let y = t[0][1] + (t[1][1] - t[0][1]) * s + (t[2][1] - t[0][1]) * r;
        best = Some(best.map_or(y, |b: f32| b.max(y)));
    }
    best
}

/// The steepest crest in the basin for a street `len` long running east or
/// north, away from the water: where the land in the middle stands highest
/// over the ends.
fn crest(w: &World, len: f32) -> (Vec2, Vec2) {
    let land = &w.landscape;
    let mut best = (f32::MIN, Vec2::ZERO, Vec2::ZERO);
    for gy in -7..=7 {
        for gx in -7..=7 {
            let c = Vec2::new(gx as f32 * 40.0, gy as f32 * 40.0);
            for dir in [Vec2::X, Vec2::Y] {
                let (a, b) = (c - dir * len * 0.5, c + dir * len * 0.5);
                let dry = (0..=16).all(|k| land.water_distance(a.lerp(b, k as f32 / 16.0)) > 60.0);
                if !dry {
                    continue;
                }
                let rise = land.height(c) - 0.5 * (land.height(a) + land.height(b));
                if rise > best.0 {
                    best = (rise, a, b);
                }
            }
        }
    }
    assert!(best.0 > 2.0, "no crest in the basin (best {:.1} m)", best.0);
    (best.1, best.2)
}

#[test]
fn terrain_stays_under_every_ground_street() {
    let mut w = World::new(WorldConfig { landscape: LandscapeParams { relief: 0.8, ..LandscapeParams::default() }, ..WorldConfig::default() });
    let (a, b) = crest(&w, 170.0);
    let dir = (b - a).normalize();
    let side = Vec2::new(-dir.y, dir.x);
    // A six-lane street over the crest, a cross street at each end, and
    // lots both sides.
    w.add_gesture(Gesture::Road { points: vec![a, b], lanes_fwd: 3, lanes_bwd: 3, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
    for e in [a, b] {
        w.add_gesture(Gesture::Road { points: vec![e - side * 80.0, e + side * 80.0], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
    }
    for s in [-1.0f32, 1.0] {
        let (n0, n1) = (side * s * 16.0, side * s * 75.0);
        let (p, q) = (a + dir * 20.0, b - dir * 20.0);
        w.add_gesture(Gesture::Zone { polygon: vec![p + n0, q + n0, q + n1, p + n1], land_use: LandUse::Residential });
    }
    w.implement();

    let mine: Vec<_> = w.roads.alive_segments().filter(|(id, _)| {
        let (p, q) = w.roads.segment_points(*id);
        project_on_segment(p, a, b).1.distance(p) < 1.0 && project_on_segment(q, a, b).1.distance(q) < 1.0
    }).map(|(id, _)| id).collect();
    assert!(!mine.is_empty(), "the street over the crest is built");
    assert!(mine.iter().any(|s| !w.road_inner[s.0 as usize].is_empty()), "the street follows the crest through inner heights");
    let lots = w.lots.values().filter(|l| mine.contains(&l.front_segment)).count();
    assert!(lots >= 4, "lots front the street ({lots})");

    let pal = Palette::default();
    let terrain = nd_procgen::terrain::build_landscape_terrain(&w, &pal, TERRAIN_CELL);
    let streets = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &pal, seed: w.config.seed, heights: &w.road_z, inner: &w.road_inner, layer: StreetLayer::Surface, closed: &[] });
    // The carriageway, fresh or worn (a cheap street's moves to the worn
    // surface).
    let tris: Vec<[[f32; 3]; 3]> = [Some(Surface::Asphalt), Surface::Asphalt.worn()]
        .into_iter()
        .flatten()
        .flat_map(|s| {
            let m = streets.get(s);
            m.indices.chunks(3).map(|t| [m.positions[t[0] as usize], m.positions[t[1] as usize], m.positions[t[2] as usize]]).collect::<Vec<_>>()
        })
        .collect();

    let mut checked = 0;
    let mut failures = Vec::new();
    for (sid, s) in w.roads.alive_segments() {
        // Every street on the ground; a sunken one meets the ground at its
        // retaining walls.
        if !s.is_street() || s.bridge || s.viaduct || s.tunnel || w.roads.node(s.a).level != 0.0 || w.roads.node(s.b).level != 0.0 {
            continue;
        }
        let (p0, p1) = w.roads.segment_points(sid);
        let len = p0.distance(p1);
        let hw = s.half_width();
        // Clear of the junctions, where other ways' pads take over.
        let trim = hw + 10.0;
        if len < 2.0 * trim + 2.0 || p0.abs().max_element().max(p1.abs().max_element()) > LAND_HALF - 20.0 {
            continue;
        }
        let d = (p1 - p0) / len;
        let n = Vec2::new(-d.y, d.x);
        let (lo, hi) = (p0.min(p1) - Vec2::splat(hw + 2.0), p0.max(p1) + Vec2::splat(hw + 2.0));
        let near: Vec<[[f32; 3]; 3]> = tris.iter().filter(|t| t.iter().any(|v| v[0] >= lo.x && v[0] <= hi.x && v[2] >= lo.y && v[2] <= hi.y)).copied().collect();
        let steps = ((len - 2.0 * trim) / 2.0).ceil() as usize;
        for k in 0..=steps {
            let along = trim + (len - 2.0 * trim) * k as f32 / steps as f32;
            let t = along / len;
            let road = w.road_height(sid, t);
            for q in -4..=4 {
                let p = p0 + d * along + n * (hw * 0.95 * q as f32 / 4.0);
                checked += 1;
                let ground = terrain_mesh_height(&terrain.positions, p);
                if ground > road + 0.02 {
                    failures.push(format!("segment {} at {p:?}: terrain {ground:.2} over the road at {road:.2}", sid.0));
                }
                // The carriageway is drawn on the road (where the cars
                // drive) and above the terrain.
                match mesh_height_at(&near, p) {
                    Some(y) if (y - road).abs() > 0.15 => failures.push(format!("segment {} at {p:?}: asphalt at {y:.2}, the road at {road:.2}", sid.0)),
                    Some(y) if ground > y => failures.push(format!("segment {} at {p:?}: terrain {ground:.2} over the asphalt at {y:.2}", sid.0)),
                    Some(_) => {}
                    None => failures.push(format!("segment {} at {p:?}: no asphalt", sid.0)),
                }
            }
        }
    }
    assert!(checked > 500, "only {checked} points checked");
    assert!(failures.is_empty(), "{} of {checked} points:\n{}", failures.len(), failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
}
