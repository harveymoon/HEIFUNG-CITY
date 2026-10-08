//! Road QA probe: generates example cities and looks for physical
//! artifacts in the street geometry numerically, printing where they are so
//! a camera can be pointed at them.
//!
//! cargo run --profile fast -p nd_procgen --example road_qa_probe -- [preset seed]...
//!
//! Checks:
//! - `marks-in-junction`: lane markings of a run inside a junction's box;
//! - `prop-on-lanes`: a street prop (pole, bin, tree, sign...) standing on
//!   the lanes of a street at its height;
//! - `kerb-on-lanes`: a kerb top over another street's lanes;
//! - `terrain-over-road`: the ground mesh above a carriageway or pavement.

use glam::Vec2;
use nd_procgen::citygen::{self, CityPreset};
use nd_procgen::palette::Palette;
use nd_procgen::street::{generate_street, StreetInput, StreetLayer};
use nd_procgen::surface::Surface;
use nd_sim::roads::{project_on_segment, LANE_WIDTH};
use nd_sim::{NodeId, World};

struct Hits {
    name: &'static str,
    at: Vec<(Vec2, String)>,
}

impl Hits {
    fn new(name: &'static str) -> Self {
        Self { name, at: Vec::new() }
    }
    fn add(&mut self, p: Vec2, why: String) {
        // One report per 6 m.
        if !self.at.iter().any(|(q, _)| q.distance(p) < 6.0) {
            self.at.push((p, why));
        }
    }
    fn print(&self) {
        println!("  {}: {}", self.name, self.at.len());
        for (p, w) in self.at.iter().take(12) {
            println!("    ({:.0}, {:.0}) {w}", p.x, p.y);
        }
    }
}

/// Is `p` on the lanes of a street at about height `y`? Returns the segment.
fn on_lanes(w: &World, p: Vec2, y: f32, margin: f32) -> Option<(u32, f32)> {
    let roads = &w.roads;
    for sid in roads.segments_in_box(p - Vec2::splat(20.0), p + Vec2::splat(20.0)) {
        let s = roads.segment(sid);
        if !s.alive || !s.is_street() || s.tunnel {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let (t, q) = project_on_segment(p, a, b);
        if !(0.0..=1.0).contains(&t) {
            continue;
        }
        let lanes = s.lanes() as f32 * LANE_WIDTH * 0.5;
        if q.distance(p) < lanes - margin {
            let z = if w.relief { w.road_height(sid, t) } else { 0.0 };
            if (y - z) > -0.5 && (y - z) < 6.0 {
                return Some((sid.0, y - z));
            }
        }
    }
    None
}

fn probe(w: &World, label: &str) {
    let pal = Palette::default();
    let heights: &[f32] = if w.relief { &w.road_z } else { &[] };
    let inner: &[Vec<(f32, f32)>] = if w.relief { &w.road_inner } else { &[] };
    let g = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &pal, seed: w.config.seed, heights, inner, layer: StreetLayer::Surface });
    let roads = &w.roads;
    let nz = |n: NodeId| heights.get(n.0 as usize).copied().unwrap_or(0.0);
    println!("{label}: {} segments, {} triangles", roads.alive_segments().count(), g.triangle_count());
    if std::env::var("ND_PLACES").is_ok() {
        let mut seen: Vec<(Vec2, &str)> = Vec::new();
        for (sid, s) in roads.alive_segments() {
            let (a, b) = roads.segment_points(sid);
            let m = a.lerp(b, 0.5);
            let kind = if s.tunnel { "tunnel" } else if s.viaduct { "viaduct" } else if s.bridge { "bridge" } else if s.ramp { "ramp" } else { continue };
            if seen.iter().any(|(q, k)| *k == kind && q.distance(m) < 150.0) || seen.iter().filter(|(_, k)| *k == kind).count() >= 6 {
                continue;
            }
            seen.push((m, kind));
            println!("  place {kind} ({:.0}, {:.0}) seg {} z {:.1}..{:.1}", m.x, m.y, sid.0, nz(s.a), nz(s.b));
        }
        for u in w.ground.underpasses.iter().take(4) {
            println!("  place underpass ({:.0}, {:.0})", u.at.x, u.at.y);
        }
    }

    // Junction boxes: streets meeting at 3+ (not a ramp's gore, not a ring).
    let mut marks = Hits::new("marks-in-junction");
    let paint = g.get(Surface::Paint);
    for (i, node) in roads.nodes.iter().enumerate() {
        let nid = NodeId(i as u32);
        let streets: Vec<_> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
        if streets.len() < 3 || roads.ring_of_node(nid).is_some() {
            continue;
        }
        if streets.iter().filter(|&&s| roads.segment(s).is_highway()).count() == 2 {
            continue;
        }
        if node.segments.iter().any(|&s| roads.segment(s).alive && roads.segment(s).is_rail()) {
            continue;
        }
        let r = w.traffic.net.node_radius.get(i).copied().unwrap_or(4.0) - 0.5;
        let z = nz(nid);
        let n = paint.positions.iter().filter(|p| Vec2::new(p[0], p[2]).distance(node.pos) < r && (p[1] - z).abs() < 0.5).count();
        if n > 0 {
            marks.add(node.pos, format!("{n} paint vertices within {r:.1} m, {} arms", streets.len()));
        }
    }
    marks.print();

    // Props standing on lanes.
    let mut props = Hits::new("prop-on-lanes");
    let mut hist: std::collections::BTreeMap<String, usize> = Default::default();
    for s in [Surface::Matte, Surface::Metal, Surface::Wood, Surface::Glass, Surface::Panel, Surface::Foliage, Surface::WornMetal] {
        let m = g.get(s);
        if let Ok(spot) = std::env::var("ND_TRIS") {
            let v: Vec<f32> = spot.split(',').map(|x| x.parse().unwrap()).collect();
            let c = Vec2::new(v[0], v[1]);
            for t in m.indices.chunks(3) {
                let ps = [t[0], t[1], t[2]].map(|i| m.positions[i as usize]);
                if ps.iter().all(|p| Vec2::new(p[0], p[2]).distance(c) < 6.0 && p[1] > 3.0 && p[1] < 3.8) {
                    println!("    tri {s:?} {:?}", ps);
                }
            }
        }
        for p in &m.positions {
            let q = Vec2::new(p[0], p[2]);
            let base = if w.relief { w.surface_height(q) } else { 0.0 };
            if p[1] - base < 0.4 {
                continue;
            }
            // Signal arms and heads hang over the lanes on purpose.
            if std::env::var("ND_ONLY").is_ok_and(|o| o != format!("{s:?}")) {
                continue;
            }
            if let Some((sid, dy)) = on_lanes(w, q, p[1], 0.3).filter(|&(_, dy)| dy > 0.2 && dy < 3.5) {
                *hist.entry(format!("{s:?} {:.1}", dy)).or_insert(0usize) += 1;
                props.add(q, format!("{s:?} vertex {dy:.1} m over the lanes of segment {sid}"));
            }
        }
    }
    let walk = nd_procgen::walk::walk_detail(w, &pal, Vec2::ZERO, 5000.0);
    for (s, m) in walk.iter() {
        if matches!(s, Surface::Paint | Surface::Asphalt | Surface::WornAsphalt) {
            continue;
        }
        for p in &m.positions {
            let q = Vec2::new(p[0], p[2]);
            if let Some((sid, dy)) = on_lanes(w, q, p[1], 0.6) {
                if dy > 0.12 && dy < 3.5 {
                    props.add(q, format!("walk {s:?} vertex {dy:.2} m over the lanes of segment {sid}"));
                }
            }
        }
    }
    props.print();
    if std::env::var("ND_HIST").is_ok() {
        println!("  {hist:?}");
    }

    // Kerb tops over lanes.
    let mut kerbs = Hits::new("kerb-on-lanes");
    for s in [Surface::Concrete, Surface::WornConcrete, Surface::Paving, Surface::WornPaving] {
        for p in &g.get(s).positions {
            let q = Vec2::new(p[0], p[2]);
            if let Some((sid, dy)) = on_lanes(w, q, p[1], 0.4) {
                if dy > 0.09 && dy < 0.3 {
                    kerbs.add(q, format!("{s:?} at {dy:.2} m over the lanes of segment {sid}"));
                }
            }
        }
    }
    kerbs.print();

    // Terrain over roads.
    if w.relief && !w.landscape.is_flat() {
        let cell = 8.0;
        let ground = nd_procgen::terrain::build_landscape_terrain(w, &pal, cell);
        let half = nd_sim::landscape::LAND_HALF;
        let n = ((half * 2.0) / cell).ceil() as usize + 1;
        let h = |p: Vec2| {
            let fx = (p.x + half) / cell;
            let fz = (p.y + half) / cell;
            let (i, j) = (fx.floor() as usize, fz.floor() as usize);
            if i + 1 >= n || j + 1 >= n {
                return f32::MIN;
            }
            let (u, v) = (fx - i as f32, fz - j as f32);
            let at = |ii: usize, jj: usize| ground.positions[jj * n + ii][1];
            // Triangles (a, c, b) and (b, c, d): split on the b-c diagonal.
            if u + v <= 1.0 {
                at(i, j) + (at(i + 1, j) - at(i, j)) * u + (at(i, j + 1) - at(i, j)) * v
            } else {
                at(i + 1, j + 1) + (at(i, j + 1) - at(i + 1, j + 1)) * (1.0 - u) + (at(i + 1, j) - at(i + 1, j + 1)) * (1.0 - v)
            }
        };
        let mut over = Hits::new("terrain-over-road");
        for (sid, s) in roads.alive_segments() {
            if !s.is_street() || s.tunnel || s.bridge || s.viaduct {
                continue;
            }
            let (a, b) = roads.segment_points(sid);
            let len = a.distance(b);
            let d = (b - a).normalize_or_zero();
            let right = Vec2::new(-d.y, d.x);
            let hw = s.half_width();
            let pave = if s.fronts_nothing() { 0.0 } else { nd_sim::zoning::SIDEWALK };
            let steps = (len / 2.0).ceil() as usize;
            for k in 1..steps {
                let t = k as f32 / steps as f32;
                let z = w.road_height(sid, t);
                for (lat, top) in [(0.0, 0.05), (hw - 0.5, 0.05), (hw + pave * 0.5, 0.15), (hw + pave - 0.2, 0.15)] {
                    if lat > hw && pave == 0.0 {
                        continue;
                    }
                    for sg in [-1.0f32, 1.0] {
                        let p = a.lerp(b, t) + right * sg * lat;
                        if p.x.abs() > half - 24.0 || p.y.abs() > half - 24.0 {
                            continue;
                        }
                        // Not where another road's deck or a junction covers it.
                        let gh = h(p);
                        if gh > z + top + 0.03 {
                            over.add(p, format!("ground {:.2} m over the surface of segment {} (lat {lat:.1})", gh - z - top, sid.0));
                        }
                    }
                }
            }
        }
        over.print();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pairs: Vec<(String, u64)> = if args.is_empty() {
        vec![("grid_plaza".into(), 1), ("coastal_river".into(), 2), ("old_town".into(), 3), ("twin_centres".into(), 4)]
    } else {
        args.chunks(2).map(|c| (c[0].clone(), c.get(1).and_then(|s| s.parse().ok()).unwrap_or(1))).collect()
    };
    for (preset, seed) in pairs {
        let Some(p) = CityPreset::parse(&preset) else {
            println!("unknown preset {preset}");
            continue;
        };
        let (w, _, _) = citygen::generate_world(p, seed);
        probe(&w, &format!("{preset} {seed}"));
    }
}
