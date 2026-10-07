//! Planned roads as they will be built: the same street generator the city
//! is drawn with (carriageway, lane lines, kerbs, pavements), at the heights
//! the road was planned at, with viaduct decks and pillars where it is
//! raised, tunnel bores where it is deep, and a pad at every junction it
//! will make. The renderer draws the result as a translucent hologram: the
//! Road tool's road while it is drawn, and every pending project's roads.

use crate::mesh::MeshData;
use crate::palette::Palette;
use crate::street::{generate_street, StreetInput, StreetLayer};
use crate::surface::{Geometry, Surface};
use glam::Vec2;
use nd_sim::roads::{is_highway_spec, RoadKind, RoadNetwork, TUNNEL_LEVEL, VIADUCT_LEVEL};
use nd_sim::traffic::LaneNetwork;
use nd_sim::World;

/// A planned road, generated.
#[derive(Default)]
pub struct RoadPreview {
    /// The road in world coordinates (heights included), one mesh, vertex
    /// colours tinted for a hologram.
    pub mesh: MeshData,
    /// Where it will meet other ways at grade (new junctions).
    pub junctions: Vec<Vec2>,
    /// Where it will pass over or under another way.
    pub flyovers: Vec<Vec2>,
    /// Height of the road at each point of the polyline it was made from.
    pub heights: Vec<f32>,
}

/// Generate the hologram of a road along `points` (the built polyline: a
/// curve already sampled) with a level per point. `steep` lists the legs
/// (index of the first point) to tint as conflicts; `tint` and `warn` are
/// the hologram and conflict colours, `alpha` its opacity.
#[allow(clippy::too_many_arguments)]
pub fn road_preview(world: &World, points: &[Vec2], levels: &[f32], lanes_fwd: u8, lanes_bwd: u8, kind: RoadKind, palette: &Palette, steep: &[usize], tint: [f32; 3], warn: [f32; 3], alpha: f32) -> RoadPreview {
    road_preview_with(world, points, levels, lanes_fwd, lanes_bwd, kind, palette, steep, tint, warn, alpha, None)
}

/// [tools] `road_preview` with the columns where the pillar plan puts them
/// (`World::road_pillar_plan`) instead of evenly spaced, and its gaps (no
/// room for a column) tinted as conflicts.
#[allow(clippy::too_many_arguments)]
pub fn road_preview_with(world: &World, points: &[Vec2], levels: &[f32], lanes_fwd: u8, lanes_bwd: u8, kind: RoadKind, palette: &Palette, steep: &[usize], tint: [f32; 3], warn: [f32; 3], alpha: f32, pillars: Option<&nd_sim::pillars::PillarPlan>) -> RoadPreview {
    let mut out = RoadPreview::default();
    if points.len() < 2 {
        return out;
    }
    let (lanes_fwd, lanes_bwd) = match kind {
        RoadKind::Street if lanes_fwd == 0 && lanes_bwd == 0 => (1, 1),
        RoadKind::Street => (lanes_fwd, lanes_bwd),
        _ => (0, 0),
    };
    let level = |i: usize| levels.get(i).copied().unwrap_or(0.0);
    // Lay it into a copy of the network to see where it joins and crosses.
    let mut net = world.roads.clone();
    let made = net.add_polyline_levels(points, levels, lanes_fwd, lanes_bwd, kind);
    let highway = is_highway_spec(kind, lanes_fwd, lanes_bwd);
    let separated = net.grade_crossings();
    for &sid in &made {
        let s = net.segment(sid);
        for n in [s.a, s.b] {
            let alive = net.node(n).segments.iter().filter(|&&o| net.segment(o).alive).count();
            let pos = net.node(n).pos;
            if alive >= 3 && !out.junctions.iter().any(|j| j.distance(pos) < 1.0) {
                out.junctions.push(pos);
            }
        }
    }
    for c in &separated {
        if (made.contains(&c.upper.0) || made.contains(&c.lower.0)) && !out.flyovers.iter().any(|f| f.distance(c.at) < 1.0) {
            out.flyovers.push(c.at);
        }
    }
    // The road alone, as a network of its own pieces.
    let mut mini = RoadNetwork::default();
    for &sid in &made {
        let s = net.segment(sid).clone();
        let (pa, pb) = (net.node(s.a).pos, net.node(s.b).pos);
        let (la, lb) = (net.node(s.a).level, net.node(s.b).level);
        let (f, b) = if s.lanes_fwd + s.lanes_bwd > 0 || kind != RoadKind::Street { (s.lanes_fwd, s.lanes_bwd) } else { (lanes_fwd, lanes_bwd) };
        mini.add_polyline_levels(&[pa, pb], &[la, lb], f, b, s.kind);
    }
    // A slip road is one for what it joins: alone, a ramp onto a highway is
    // just a one-way street, and was drawn with kerbs and pavements, far
    // wider than it will be built.
    let ramps: Vec<(Vec2, Vec2)> = made.iter().filter(|&&sid| net.segment(sid).ramp).map(|&sid| net.segment_points(sid)).collect();
    for i in 0..mini.segments.len() {
        let (a, b) = mini.segment_points(nd_sim::SegmentId(i as u32));
        if ramps.iter().any(|&(p, q)| (p.distance(a) < 0.01 && q.distance(b) < 0.01) || (p.distance(b) < 0.01 && q.distance(a) < 0.01)) {
            mini.segments[i].ramp = true;
        }
    }
    // Heights: the ground plus the level; where it lands on a way at grade,
    // that way's height.
    let ground = |p: Vec2| world.ground_height(p).max(if world.landscape.is_flat() { f32::MIN } else { nd_sim::landscape::WATER_LEVEL });
    let mut z: Vec<f32> = Vec::with_capacity(mini.nodes.len());
    for n in &mini.nodes {
        let on_road = world.roads.nearest_where(n.pos, |s| !s.is_rail()).filter(|(_, _, q)| q.distance(n.pos) < 1.0);
        let h = match on_road {
            Some((sid, t, _)) if !nd_sim::roads::grade_separated(world.roads.segment(sid).is_highway(), world.roads.segment_level(sid, t), highway, n.level) => world.road_height(sid, t),
            _ => ground(n.pos) + n.level,
        };
        z.push(h);
    }
    let ids: Vec<nd_sim::SegmentId> = mini.alive_segments().map(|(id, _)| id).collect();
    for id in ids {
        let (a, b) = (mini.segment(id).a, mini.segment(id).b);
        let (la, lb) = (mini.node(a).level, mini.node(b).level);
        let s = &mut mini.segments[id.0 as usize];
        s.tunnel = la.max(lb) <= TUNNEL_LEVEL;
        s.viaduct = la.max(lb) >= VIADUCT_LEVEL;
        s.bridge = s.viaduct;
    }
    out.heights = points.iter().enumerate().map(|(i, &p)| mini.node_near(p, 0.5).map_or(ground(p) + level(i), |n| z[n.0 as usize])).collect();
    let mut lanes = LaneNetwork::default();
    lanes.rebuild(&mini);
    let mut geo = generate_street(&StreetInput { roads: &mini, net: &lanes, land_value: &world.land_value, palette, seed: 1, heights: &z, inner: &[], layer: StreetLayer::All });
    // Decks and pillars under the raised parts.
    let concrete = crate::buildings::parts::mul(palette.concrete(0), 0.85);
    for (sid, s) in mini.alive_segments() {
        if !s.viaduct {
            continue;
        }
        let (a, b) = mini.segment_points(sid);
        let hw = s.half_width() + if s.is_street() { nd_sim::zoning::SIDEWALK } else { 0.6 };
        crate::terrain::viaduct_span(&mut geo, a, b, z[s.a.0 as usize], z[s.b.0 as usize], hw, &ground, concrete, palette.steel, pillars.is_none());
    }
    // [tools] The planned columns, at the deck's height over each.
    if let Some(plan) = pillars {
        for p in &plan.pillars {
            let deck = mini
                .alive_segments()
                .filter(|(_, s)| s.viaduct)
                .map(|(sid, s)| {
                    let (a, b) = mini.segment_points(sid);
                    let (t, q) = nd_sim::roads::project_on_segment(p.at, a, b);
                    (z[s.a.0 as usize] + (z[s.b.0 as usize] - z[s.a.0 as usize]) * t, q.distance(p.at))
                })
                .min_by(|x, y| x.1.total_cmp(&y.1));
            let top = deck.map_or(ground(p.at) + p.level, |d| d.0) - 1.2;
            crate::terrain::pillar_column(&mut geo, p.at, p.dir, top, ground(p.at), p.half_width, concrete);
        }
    }
    let gaps: Vec<(Vec2, Vec2)> = pillars.map(|p| p.gaps.iter().map(|g| (g.a, g.b)).collect()).unwrap_or_default();
    // Columns that buy out a building stand out in orange.
    let spots: Vec<Vec2> = pillars
        .map(|plan| plan.pillars.iter().filter(|p| plan.buyouts.iter().any(|(lot, _)| world.lots.get(*lot).is_some_and(|l| l.contains(p.at) || nd_sim::geom::distance_to_outline(p.at, &l.polygon) < nd_sim::pillars::PILLAR_RADIUS))).map(|p| p.at).collect())
        .unwrap_or_default();
    // Pads where it will make junctions.
    let mut pads = MeshData::default();
    for &j in &out.junctions {
        let h = mini.node_near(j, 1.0).map_or(ground(j), |n| z[n.0 as usize]);
        let r = 6.0 + lanes_fwd.max(lanes_bwd) as f32 * 1.5;
        let poly: Vec<Vec2> = (0..20).map(|k| j + Vec2::from_angle(k as f32 / 20.0 * std::f32::consts::TAU) * r).collect();
        pads.ground_polygon(&poly, h + 0.3, [1.0, 1.0, 1.0, 1.0]);
    }
    out.mesh = tint_hologram(&geo, &pads, points, steep, &gaps, &spots, tint, warn, alpha);
    out
}

/// One translucent mesh from the generated road: markings and lights
/// bright, surfaces dim, everything near a steep leg or a stretch with no
/// room for a column (`gaps`) in the warning colour.
#[allow(clippy::too_many_arguments)]
fn tint_hologram(geo: &Geometry, pads: &MeshData, points: &[Vec2], steep: &[usize], gaps: &[(Vec2, Vec2)], spots: &[Vec2], tint: [f32; 3], warn: [f32; 3], alpha: f32) -> MeshData {
    const BUYOUT: [f32; 3] = [1.7, 0.75, 0.1];
    let mut out = MeshData::default();
    let near_steep = |p: Vec2| {
        steep.iter().any(|&i| {
            let (Some(&a), Some(&b)) = (points.get(i), points.get(i + 1)) else { return false };
            nd_sim::roads::project_on_segment(p, a, b).1.distance(p) < 14.0
        }) || gaps.iter().any(|&(a, b)| nd_sim::roads::project_on_segment(p, a, b).1.distance(p) < 12.0)
    };
    let mut add = |m: &MeshData, bright: f32, a: f32| {
        let base = out.positions.len();
        out.append(m);
        for k in base..out.positions.len() {
            let p = out.positions[k];
            let q = Vec2::new(p[0], p[2]);
            let c = if near_steep(q) {
                warn
            } else if p[1] > 0.2 && spots.iter().any(|s| s.distance(q) < 3.2) {
                BUYOUT
            } else {
                tint
            };
            let lum = { let s = m.colors.get(k - base).copied().unwrap_or([1.0; 4]); (s[0] + s[1] + s[2]) / 3.0 };
            let f = bright * (0.55 + lum * 0.6);
            out.colors[k] = [c[0] * f, c[1] * f, c[2] * f, a];
        }
    };
    for (s, m) in geo.iter() {
        let glow = s.is_glow() || s == Surface::Glow;
        add(m, if glow { 1.6 } else { 0.7 }, if glow { (alpha * 1.6).min(0.95) } else { alpha });
    }
    add(pads, 1.4, (alpha * 0.9).min(0.9));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::{Gesture, WorldConfig};

    fn world() -> World {
        let mut w = World::new(WorldConfig { landscape: nd_sim::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        w
    }

    #[test]
    fn the_preview_is_finite_and_stands_where_the_road_will() {
        let pal = Palette::default();
        let pts = [Vec2::new(60.0, -300.0), Vec2::new(60.0, -150.0), Vec2::new(60.0, 150.0), Vec2::new(60.0, 300.0)];
        let levels = [0.0, 8.0, 8.0, 0.0];
        let mut w = world();
        let pv = road_preview(&w, &pts, &levels, 1, 1, RoadKind::Street, &pal, &[], [0.3, 0.6, 1.0], [1.0, 0.2, 0.2], 0.5);
        assert!(pv.mesh.positions.len() > 100);
        assert_eq!(pv.mesh.positions.len(), pv.mesh.colors.len());
        assert!(pv.mesh.positions.iter().all(|p| p.iter().all(|v| v.is_finite())));
        assert!(pv.mesh.indices.iter().all(|&i| (i as usize) < pv.mesh.positions.len()));
        assert!(pv.junctions.is_empty(), "it passes over");
        assert_eq!(pv.flyovers.len(), 1);
        // Something stands up at deck height over the crossing.
        assert!(pv.mesh.positions.iter().any(|p| (p[0] - 60.0).abs() < 4.0 && p[2].abs() < 4.0 && p[1] > 7.5));
        // Built, the road stands where the preview drew it.
        w.add_gesture(Gesture::Road { points: pts.to_vec(), lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: levels.to_vec() });
        w.implement();
        for (i, p) in pts.iter().enumerate() {
            let n = w.roads.node_near(*p, 0.5).unwrap();
            assert!((w.road_z[n.0 as usize] - pv.heights[i]).abs() < 0.5, "point {i}: built {} previewed {}", w.road_z[n.0 as usize], pv.heights[i]);
        }
        // At grade it makes a junction, and the pad shows it.
        let pv = road_preview(&w, &[Vec2::new(-60.0, -100.0), Vec2::new(-60.0, 100.0)], &[], 1, 1, RoadKind::Street, &pal, &[0], [0.3, 0.6, 1.0], [1.0, 0.2, 0.2], 0.5);
        assert_eq!(pv.junctions.len(), 1);
        assert!(pv.mesh.colors.iter().any(|c| c[0] > c[2]), "the steep leg is tinted");
    }
}
