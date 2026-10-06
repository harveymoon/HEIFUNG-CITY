//! Streets: carriageway, kerbs and pavements, markings and crossings,
//! junction pads, lamps, signal posts, bins, benches, shelters, bollards,
//! utility poles with sagging wires, and trees where the land is worth it.
//!
//! Roads are drawn as *runs*: chains of segments joined at plain bends
//! (nodes with exactly two roads of the same kind). A run gets one
//! continuous, mitred carriageway and pavement, so a curved road reads as
//! one road instead of a string of pieces.

use crate::buildings::parts::{bench_world, bin_world, bollard_world, lamp_post_world, mul, pick, railway_track, rgba, tree_world, CHARCOAL, DARK};
use crate::mesh::{offset_path, path_length, point_along};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::land_value::LandValue;
use nd_sim::rng_for;
use nd_sim::roads::{RoadKind, RoadNetwork, SegmentId, LANE_WIDTH, PATH_HALF_WIDTH, RAIL_HALF_WIDTH, SHOULDER};
use nd_sim::traffic::{LaneId, LaneKind, LaneNetwork};
use nd_sim::zoning::SIDEWALK;
use nd_sim::NodeId;
use rand::Rng;
use std::collections::{BTreeMap, HashSet};

pub struct StreetInput<'a> {
    pub roads: &'a RoadNetwork,
    pub net: &'a LaneNetwork,
    pub land_value: &'a LandValue,
    pub palette: &'a Palette,
    pub seed: u64,
    /// Height of every road node (`World::road_z`); empty for a flat city.
    /// Each run is set on its own profile, so a viaduct is drawn at its
    /// height and the street under it at the ground's, whatever overlaps.
    pub heights: &'a [f32],
    /// Which roads to draw: all, those on and above the ground, or only the
    /// tunnels (shown on their own in the underground view).
    pub layer: StreetLayer,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StreetLayer {
    #[default]
    All,
    Surface,
    Underground,
}

impl StreetLayer {
    fn keeps(self, tunnel: bool) -> bool {
        match self {
            StreetLayer::All => true,
            StreetLayer::Surface => !tunnel,
            StreetLayer::Underground => tunnel,
        }
    }
}

/// Where a signal lamp hangs over the end of a lane, for the renderer to
/// swap its colour at runtime.
pub struct SignalPost {
    pub lane: LaneId,
    pub lamp: Vec3,
    pub heading: f32,
}

const KERB_H: f32 = 0.16;
const ASPHALT_Y: f32 = 0.05;
const MARK_Y: f32 = 0.085;

/// A chain of segments drawn as one road.
struct Run {
    points: Vec<Vec2>,
    /// The road node at each point.
    nodes: Vec<NodeId>,
    first: SegmentId,
    lanes_fwd: u8,
    lanes_bwd: u8,
    kind: RoadKind,
    start_node: NodeId,
    end_node: NodeId,
}

/// Roads of one kind at a node.
fn degree_of(roads: &RoadNetwork, n: NodeId, kind: RoadKind) -> usize {
    roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive && roads.segment(s).kind == kind).count()
}

fn degree(roads: &RoadNetwork, n: NodeId) -> usize {
    roads.street_degree(n)
}

/// Oriented lanes of a segment when travelling from `from` to its far node.
fn oriented(roads: &RoadNetwork, sid: SegmentId, from: NodeId) -> (NodeId, u8, u8) {
    let s = roads.segment(sid);
    if s.a == from {
        (s.b, s.lanes_fwd, s.lanes_bwd)
    } else {
        (s.a, s.lanes_bwd, s.lanes_fwd)
    }
}

/// The segment continuing past a bend node, if the node is a plain bend of
/// the same road kind.
fn continuation(roads: &RoadNetwork, node: NodeId, from_seg: SegmentId, lanes_fwd: u8, lanes_bwd: u8) -> Option<SegmentId> {
    let kind = roads.segment(from_seg).kind;
    if degree_of(roads, node, kind) != 2 {
        return None;
    }
    let next = roads.node(node).segments.iter().copied().find(|&s| s != from_seg && roads.segment(s).alive && roads.segment(s).kind == kind)?;
    let (_, f, b) = oriented(roads, next, node);
    // A tunnel is a run of its own: it starts at the portal.
    (f == lanes_fwd && b == lanes_bwd && roads.segment(next).tunnel == roads.segment(from_seg).tunnel).then_some(next)
}

fn build_runs(roads: &RoadNetwork) -> Vec<Run> {
    let mut used: HashSet<u32> = HashSet::new();
    let mut runs = Vec::new();
    for (sid, seg) in roads.alive_segments() {
        if used.contains(&sid.0) {
            continue;
        }
        // Walk backwards to the start of the chain.
        let (mut start_seg, mut start_node) = (sid, seg.a);
        let (mut lf, mut lb) = (seg.lanes_fwd, seg.lanes_bwd);
        let mut guard = 0;
        while let Some(prev) = continuation(roads, start_node, start_seg, lb, lf) {
            if prev == sid || guard > 10_000 {
                break; // a loop: start anywhere
            }
            guard += 1;
            let (far, f, b) = oriented(roads, prev, start_node);
            // `prev` is traversed backwards here; its forward lanes toward
            // start_node are what the run needs.
            lf = b;
            lb = f;
            start_seg = prev;
            start_node = far;
        }
        // Walk forwards collecting points.
        let mut points = vec![roads.node(start_node).pos];
        let mut nodes = vec![start_node];
        let mut node = start_node;
        let mut cur = start_seg;
        let (_, run_f, run_b) = oriented(roads, cur, node);
        let first = cur;
        loop {
            used.insert(cur.0);
            let (far, _, _) = oriented(roads, cur, node);
            points.push(roads.node(far).pos);
            nodes.push(far);
            node = far;
            match continuation(roads, node, cur, run_f, run_b) {
                Some(next) if !used.contains(&next.0) => cur = next,
                _ => break,
            }
        }
        runs.push(Run { points, nodes, first, lanes_fwd: run_f, lanes_bwd: run_b, kind: roads.segment(first).kind, start_node, end_node: node });
    }
    runs
}

/// Trim a polyline by `start` at the beginning and `end` at the end.
fn trim(points: &[Vec2], start: f32, end: f32) -> Vec<Vec2> {
    let total = path_length(points);
    if total - start - end < 1.0 {
        return Vec::new();
    }
    let mut out = vec![point_along(points, start).0];
    let mut acc = 0.0;
    for w in points.windows(2) {
        acc += w[0].distance(w[1]);
        if acc > start && acc < total - end {
            out.push(w[1]);
        }
    }
    out.push(point_along(points, total - end).0);
    out
}

/// Arrows painted along a lane every 45 m (none within 12 m of its ends),
/// pointing the way traffic runs: `forward` along the path.
pub fn travel_arrows(m: &mut crate::mesh::MeshData, path: &[Vec2], forward: bool, color: [f32; 4]) {
    let len = path_length(path);
    let mut s = 20.0;
    while s < len - 12.0 {
        let (p, d) = point_along(path, s);
        let d = if forward { d } else { -d };
        let side = Vec2::new(-d.y, d.x);
        let tail = p - d * 2.0;
        let tip = p + d * 2.2;
        m.ground_strip(&[tail, p + d * 0.9], 0.22, MARK_Y + 0.003, color);
        for sgn in [-1.0f32, 1.0] {
            m.ground_strip(&[p + d * 0.6 + side * sgn * 0.75, tip], 0.22, MARK_Y + 0.003, color);
        }
        s += 45.0;
    }
}

/// Where a ramp joins a highway (or leaves it): asphalt filling the taper
/// between the ramp's edge and the carriageway's until they part, edge
/// lines along both, and chevron hatching in the gore beyond.
fn ramp_gore(g: &mut Geometry, roads: &RoadNetwork, node: NodeId, ramp: SegmentId, alive: &[SegmentId], asphalt: [f32; 4], paint: [f32; 4]) {
    let at = roads.node(node).pos;
    let away = |s: SegmentId| {
        let seg = roads.segment(s);
        let far = if seg.a == node { seg.b } else { seg.a };
        (roads.node(far).pos - at).normalize_or_zero()
    };
    let dr = away(ramp);
    // The carriageway on the ramp's side of the node: the one it runs along.
    let Some(&hw_seg) = alive.iter().filter(|&&s| s != ramp).max_by(|&&x, &&y| away(x).dot(dr).total_cmp(&away(y).dot(dr))) else { return };
    let dh = away(hw_seg);
    let (hh, hr) = (roads.segment(hw_seg).half_width() - SHOULDER * 0.5, roads.segment(ramp).half_width() - SHOULDER * 0.5);
    // Unit normals of each way toward the other.
    let nh = { let n = Vec2::new(-dh.y, dh.x); if n.dot(dr) >= 0.0 { n } else { -n } };
    let nr = { let n = Vec2::new(-dr.y, dr.x); if n.dot(dh) >= 0.0 { n } else { -n } };
    let mut fill: Vec<Vec2> = Vec::new();
    let mut back: Vec<Vec2> = Vec::new();
    let mut parted = None;
    let mut s = 0.0f32;
    while s <= 60.0 {
        let eh = at + dh * s + nh * hh;
        let er = at + dr * s + nr * hr;
        // The two edges cross while the ramp still overlaps the carriageway;
        // once the ramp's inner edge is outside, the gap between is the gore.
        let gap = (er - eh).dot(nh);
        if gap > 0.15 {
            parted.get_or_insert(s);
            if s - parted.unwrap() > 40.0 {
                break;
            }
            if fill.len() < 2 {
                fill.push(eh);
                back.push(er);
            }
            // Chevrons across the gore every 3 m.
            if ((s - parted.unwrap()) / 3.0).fract() < 0.34 && gap > 0.8 {
                let mid = (eh + er) * 0.5 + (dh + dr).normalize_or_zero() * 1.2;
                g.part(Glow).ground_strip(&[eh + (er - eh) * 0.08, mid], 0.3, MARK_Y + 0.004, paint);
                g.part(Glow).ground_strip(&[mid, er - (er - eh) * 0.08], 0.3, MARK_Y + 0.004, paint);
            }
        } else {
            fill.push(eh);
            back.push(er);
        }
        s += 1.0;
    }
    if fill.len() >= 2 {
        let mut poly = fill.clone();
        poly.extend(back.iter().rev());
        g.part(Asphalt).ground_polygon(&poly, ASPHALT_Y + 0.004, asphalt);
        // Edge lines along the taper on both sides of the gore.
        g.part(Glow).ground_strip(&fill, 0.18, MARK_Y + 0.002, paint);
        g.part(Glow).ground_strip(&back, 0.18, MARK_Y + 0.002, paint);
    }
}

/// Vertex counts of every part: where the next piece of drawing starts.
fn marks(g: &Geometry) -> Vec<usize> {
    g.parts.iter().map(|m| m.positions.len()).collect()
}

/// Raise every vertex drawn since `from` by `h` of its (x, z).
fn lift_since(g: &mut Geometry, from: &[usize], h: impl Fn(Vec2) -> f32) {
    for (k, m) in g.parts.iter_mut().enumerate() {
        let start = from.get(k).copied().unwrap_or(0);
        for p in m.positions.iter_mut().skip(start) {
            p[1] += h(Vec2::new(p[0], p[2]));
        }
    }
}

/// Height along a run at the point of it nearest `p`, from the heights of
/// its nodes.
pub fn profile_height(points: &[Vec2], z: &[f32], p: Vec2) -> f32 {
    if points.len() < 2 || z.len() != points.len() {
        return z.first().copied().unwrap_or(0.0);
    }
    let mut best = (f32::INFINITY, 0.0f32);
    for (i, w) in points.windows(2).enumerate() {
        let (t, q) = nd_sim::roads::project_on_segment(p, w[0], w[1]);
        let d = q.distance_squared(p);
        if d < best.0 {
            best = (d, z[i] + (z[i + 1] - z[i]) * t);
        }
    }
    best.1
}

fn dashes_along(mesh: &mut crate::mesh::MeshData, path: &[Vec2], width: f32, y: f32, dash: f32, gap: f32, color: [f32; 4]) {
    let len = path_length(path);
    let mut t = gap * 0.5;
    while t + dash <= len {
        let (a, _) = point_along(path, t);
        let (b, _) = point_along(path, t + dash);
        mesh.ground_strip(&[a, b], width, y, color);
        t += dash + gap;
    }
}

pub fn generate_street(inp: &StreetInput) -> Geometry {
    let mut g = Geometry::default();
    let pal = inp.palette;
    let roads = inp.roads;
    let net = inp.net;
    let asphalt_c = rgba(pal.asphalt);
    let edge_c = rgba(pal.lane_marker);
    let centre_c = rgba(pal.centre_line);
    let divider_c = rgba(pal.lane_divider);
    let paving_c = rgba(pal.paving);
    let kerb_c = rgba(pal.curb);
    let node_r = |n: NodeId| net.node_radius.get(n.0 as usize).copied().unwrap_or(4.0);
    // Where a neglected run started in each weathered surface's mesh: its
    // carriageway, kerbs and slabs move onto the worn materials once drawn.
    let mut worn_from: Option<Vec<(Surface, usize, usize)>> = None;
    let weather_tail = |g: &mut Geometry, marks: Vec<(Surface, usize, usize)>| {
        for (s, v, i) in marks {
            if let Some(w) = s.worn() {
                let tail = g.part(s).take_from(v, i);
                g.part(w).append(&tail);
            }
        }
    };

    let heights = inp.heights;
    let node_z = |n: NodeId| heights.get(n.0 as usize).copied().unwrap_or(0.0);
    // The run drawn last and where its drawing began, to set it on its
    // profile once it is complete (runs end at several `continue`s).
    let mut drawn: Option<(Vec<usize>, Vec<Vec2>, Vec<f32>)> = None;
    let finish_run = |g: &mut Geometry, drawn: &mut Option<(Vec<usize>, Vec<Vec2>, Vec<f32>)>| {
        if let Some((from, pts, z)) = drawn.take() {
            if z.iter().any(|h| *h != 0.0) {
                lift_since(g, &from, |p| profile_height(&pts, &z, p));
            }
        }
    };
    for run in build_runs(roads) {
        finish_run(&mut g, &mut drawn);
        if let Some(marks) = worn_from.take() {
            weather_tail(&mut g, marks);
        }
        if !heights.is_empty() {
            drawn = Some((marks(&g), run.points.clone(), run.nodes.iter().map(|&n| node_z(n)).collect()));
        }
        let seg = roads.segment(run.first);
        if !inp.layer.keeps(seg.tunnel) {
            continue;
        }
        let hw = seg.half_width();
        let total = run.lanes_fwd as f32 + run.lanes_bwd as f32;
        let mut rng = rng_for(inp.seed ^ (run.first.0 as u64 * 7919), 3);
        let pts = &run.points;
        if path_length(pts) < 0.5 {
            continue;
        }
        if run.kind == RoadKind::Path {
            footpath(&mut g, pal, roads, &run, &mut rng);
            continue;
        }
        if run.kind == RoadKind::Rail {
            railway(&mut g, pal, &run);
            continue;
        }
        // Cheap streets are neglected: faded markings, potholes, cracked
        // slabs and litter. The land value in the middle of the run decides.
        let lv_mid = inp.land_value.at(point_along(pts, path_length(pts) * 0.5).0);
        let grit = lv_mid < 0.35;
        if grit {
            worn_from = Some([Asphalt, Paving, Concrete].iter().map(|s| (*s, g.get(*s).positions.len(), g.get(*s).indices.len())).collect());
        }
        let fade = |c: [f32; 4]| if grit { [c[0] * 0.5, c[1] * 0.5, c[2] * 0.5, c[3]] } else { c };
        // Carriageway and markings run the whole way; the junction pads overlap the ends.
        g.part(Asphalt).path_strip(pts, 0.0, hw * 2.0, ASPHALT_Y, asphalt_c);
        g.part(Glow).path_strip(pts, hw - SHOULDER * 0.5, 0.18, MARK_Y, fade(edge_c));
        g.part(Glow).path_strip(pts, -(hw - SHOULDER * 0.5), 0.18, MARK_Y, fade(edge_c));
        for k in 1..(run.lanes_fwd + run.lanes_bwd) as u32 {
            let off = (k as f32 - total * 0.5) * LANE_WIDTH;
            let is_centre = run.lanes_fwd > 0 && run.lanes_bwd > 0 && k == run.lanes_bwd as u32;
            if is_centre {
                g.part(Glow).path_strip(pts, off + 0.18, 0.14, MARK_Y, fade(centre_c));
                g.part(Glow).path_strip(pts, off - 0.18, 0.14, MARK_Y, fade(centre_c));
            } else {
                let path = offset_path(pts, off);
                dashes_along(g.part(Glow), &path, 0.16, MARK_Y, 3.0, 4.0, fade(divider_c));
            }
        }
        // One-way roads (ramps, highways, one-way streets) show which way
        // they run: an arrow in every lane every 45 m.
        if (run.lanes_fwd == 0) != (run.lanes_bwd == 0) {
            let forward = run.lanes_fwd > 0;
            for k in 0..(run.lanes_fwd + run.lanes_bwd) {
                let path = offset_path(pts, (k as f32 + 0.5 - total * 0.5) * LANE_WIDTH);
                travel_arrows(g.part(Glow), &path, forward, fade(edge_c));
            }
        }

        // Kerbs and pavements stop short of the junction pads.
        let d_start = degree(roads, run.start_node);
        let d_end = degree(roads, run.end_node);
        let t0 = node_r(run.start_node) + 0.6;
        let t1 = node_r(run.end_node) + 0.6;
        let walk = trim(pts, t0, t1);
        if walk.len() < 2 {
            continue;
        }
        for side in [-1.0f32, 1.0] {
            g.part(Concrete).path_box(&walk, side * (hw + 0.15), 0.3, 0.0, KERB_H, kerb_c);
            let pw = SIDEWALK - 0.3;
            g.part(Paving).path_box(&walk, side * (hw + 0.3 + pw * 0.5), pw, 0.0, KERB_H - 0.01, paving_c);
        }

        if seg.tunnel {
            // The bore: walls past the pavements, a lit strip overhead, and
            // no street furniture.
            let wall = rgba(mul(pal.concrete(0), 0.7));
            for side in [-1.0f32, 1.0] {
                g.part(Concrete).path_box(&walk, side * (hw + SIDEWALK + 0.2), 0.4, 0.0, 5.6, wall);
            }
            g.part(Glow).path_box(&walk, 0.0, 0.25, 5.3, 5.4, rgba(mul(pal.street_lamp, 0.9)));
            continue;
        }
        // Furniture by arc length along the pavement.
        let len = path_length(&walk);
        let lamp_off = hw + 0.95;
        let back_off = hw + SIDEWALK - 0.7;
        let at = |s: f32, side: f32, off: f32| {
            let (p, d) = point_along(&walk, s);
            let right = Vec2::new(-d.y, d.x);
            (p + right * side * off, d, right)
        };
        let mut lamp_ts: Vec<(f32, f32)> = Vec::new();
        let mut s = 6.0;
        let mut side = 1.0;
        while s < len - 4.0 {
            let (p, _, _) = at(s, side, lamp_off);
            lamp_post_world(&mut g, p, 7.0, pal.street_lamp);
            lamp_ts.push((s, side));
            s += 28.0;
            side = -side;
        }
        if lv_mid > 0.42 {
            let mut s = 4.0;
            while s < len - 3.0 {
                for side in [-1.0f32, 1.0] {
                    if lamp_ts.iter().any(|(lt, ls)| *ls == side && (lt - s).abs() < 3.0) {
                        continue;
                    }
                    let (p, d, _) = at(s, side, back_off);
                    g.part(Concrete).boxed(Vec3::new(p.x, KERB_H + 0.2, p.y), Vec3::new(1.3, 0.4, 1.3), d.y.atan2(d.x), rgba(mul(pal.paving, 1.1)));
                    tree_world(&mut g, p, rng.random_range(3.0..4.5), rng.random_range(2.8..4.0), pal.bark, mul(pal.foliage, rng.random_range(0.85..1.15)));
                }
                s += 13.0 + rng.random_range(0.0..3.0);
            }
        }
        if lv_mid < 0.4 {
            let mut prev: Option<Vec3> = None;
            let mut s = 2.0;
            while s < len - 1.0 {
                let (p, d, right) = at(s, -1.0, back_off);
                g.part(Wood).cylinder(p, 0.16, 0.12, 0.0, 8.5, 6, rgba(pal.bark));
                g.part(Wood).boxed(Vec3::new(p.x, 8.0, p.y), Vec3::new(0.1, 0.1, 1.6), d.y.atan2(d.x), rgba(pal.bark));
                let top = Vec3::new(p.x, 8.2, p.y);
                if let Some(q) = prev {
                    for dz in [-0.6f32, 0.0, 0.6] {
                        let off = Vec3::new(right.x, 0.0, right.y) * dz;
                        g.part(Matte).wire(q + off, top + off, 0.8, 0.035, rgba(DARK));
                    }
                }
                if rng.random_bool(0.3) {
                    g.part(Metal).boxed(Vec3::new(p.x, 7.0, p.y) + Vec3::new(right.x, 0.0, right.y) * 0.35, Vec3::new(0.5, 0.9, 0.5), 0.0, rgba(mul(pal.steel, 0.6)));
                }
                prev = Some(top);
                s += 32.0;
            }
        }
        let mut s = 10.0 + rng.random_range(0.0..8.0);
        while s < len - 6.0 {
            let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
            let (p, d, right) = at(s, side, hw + 1.2);
            match rng.random_range(0..6) {
                0 | 1 => bin_world(&mut g, p, mul(pal.metal_paint(0), 0.9)),
                2 if lv_mid > 0.35 => {
                    let (bp, _, _) = at(s, side, back_off);
                    bench_world(&mut g, bp, d.y.atan2(d.x) + if side > 0.0 { std::f32::consts::PI } else { 0.0 }, pal.wood, CHARCOAL);
                }
                3 if lv_mid < 0.55 => {
                    let (kp, _, _) = at(s, side, hw + 1.4);
                    let yaw = d.y.atan2(d.x);
                    g.part(Metal).boxed(Vec3::new(kp.x, 1.2, kp.y), Vec3::new(1.0, 2.4, 0.9), yaw, rgba(pal.metal_paint(2)));
                    let face = kp - right * side * 0.47;
                    let out = Vec3::new(-right.x, 0.0, -right.y) * side;
                    g.part(Glow).decal(Vec3::new(face.x, 1.5, face.y), Vec3::new(d.x, 0.0, d.y), Vec3::Y, 0.6, 0.8, out, rgba(mul(pal.led(1), 0.5)));
                }
                4 if len > 80.0 => {
                    // A phone box where a shelter used to stand at random;
                    // shelters now mark real bus stops.
                    let (sp, d, _) = at(s, side, hw + SIDEWALK * 0.55);
                    let yaw = d.y.atan2(d.x);
                    g.part(Glass).boxed(Vec3::new(sp.x, 1.2, sp.y), Vec3::new(1.0, 2.4, 1.0), yaw, rgba(pal.glass(0)));
                    g.part(Panel).boxed(Vec3::new(sp.x, 2.45, sp.y), Vec3::new(1.1, 0.1, 1.1), yaw, rgba(mul(pal.steel, 0.8)));
                }
                _ => {}
            }
            s += 22.0 + rng.random_range(0.0..20.0);
        }
        if grit {
            street_grit(&mut g, pal, &walk, hw, len, &mut rng);
        }
        // Bollards where the pavement meets a junction.
        for (s_end, deg) in [(0.0, d_start), (len, d_end)] {
            if deg < 3 {
                continue;
            }
            for side in [-1.0f32, 1.0] {
                for k in 0..2 {
                    let (q, _, _) = at(s_end, side, hw + 0.9 + k as f32 * 1.1);
                    bollard_world(&mut g, q, mul(pal.steel, 0.7));
                }
            }
        }
    }

    finish_run(&mut g, &mut drawn);
    if let Some(marks) = worn_from.take() {
        weather_tail(&mut g, marks);
    }

    // Roundabouts: a mountable apron inside the ring lanes; the middle is
    // open land. At the height of the ring's nodes.
    for ring in roads.rings.iter().filter(|_| inp.layer != StreetLayer::Underground) {
        let from = marks(&g);
        roundabout_apron(&mut g, pal, ring);
        if !heights.is_empty() {
            let on: Vec<f32> = roads.nodes.iter().enumerate().filter(|(_, n)| ring.on_circle(n.pos)).map(|(i, _)| node_z(NodeId(i as u32))).collect();
            let z = if on.is_empty() { 0.0 } else { on.iter().sum::<f32>() / on.len() as f32 };
            lift_since(&mut g, &from, |_| z);
        }
    }

    // Junction pads, crossings and stop lines (bends need none), each at
    // its node's height.
    let mut node_from: Option<(Vec<usize>, f32)> = None;
    for (i, node) in roads.nodes.iter().enumerate() {
        if let Some((from, z)) = node_from.take() {
            if z != 0.0 {
                lift_since(&mut g, &from, |_| z);
            }
        }
        if !heights.is_empty() {
            node_from = Some((marks(&g), node_z(NodeId(i as u32))));
        }
        let tunnel_node = node.segments.iter().any(|&s| roads.segment(s).alive) && node.segments.iter().filter(|&&s| roads.segment(s).alive).all(|&s| roads.segment(s).tunnel);
        if !inp.layer.keeps(tunnel_node) {
            continue;
        }
        let alive: Vec<SegmentId> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
        let paths_here = node.segments.iter().filter(|&&s| roads.segment(s).alive && roads.segment(s).kind == RoadKind::Path).count();
        let rails_here: Vec<SegmentId> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_rail()).collect();
        if !alive.is_empty() && !rails_here.is_empty() {
            level_crossing(&mut g, pal, roads, node.pos, &alive, &rails_here);
            continue;
        }
        if alive.len() == 2 && paths_here > 0 {
            // A path crossing a street: zebra across the carriageway.
            let sid = alive[0];
            let seg = roads.segment(sid);
            let (a, b) = roads.segment_points(sid);
            let d = (b - a).normalize_or_zero();
            let right = Vec2::new(-d.y, d.x);
            let hw = seg.half_width();
            let c0 = node.pos - d * 1.3;
            let c1 = node.pos + d * 1.3;
            let mut s = -hw + SHOULDER + 0.4;
            while s < hw - SHOULDER - 0.2 {
                g.part(Glow).ground_strip(&[c0 + right * s, c1 + right * s], 0.5, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 0.9)));
                s += 1.0;
            }
            continue;
        }
        if alive.is_empty() || alive.len() == 2 {
            continue;
        }
        // A ramp meeting a highway: a taper and a hatched gore instead of a
        // junction pad, crossings and stop lines.
        if alive.len() == 3 && alive.iter().filter(|&&s| roads.segment(s).is_highway()).count() == 2 {
            if let Some(&ramp) = alive.iter().find(|&&s| !roads.segment(s).is_highway()) {
                ramp_gore(&mut g, roads, NodeId(i as u32), ramp, &alive, asphalt_c, rgba(mul(pal.lane_marker, 0.9)));
                continue;
            }
        }
        let r = net.node_radius.get(i).copied().unwrap_or(4.0) + 0.3;
        let poly: Vec<Vec2> = (0..20)
            .map(|k| {
                let a = k as f32 / 20.0 * std::f32::consts::TAU;
                node.pos + Vec2::new(a.cos(), a.sin()) * r
            })
            .collect();
        g.part(Asphalt).ground_polygon(&poly, ASPHALT_Y + 0.01, asphalt_c);
        if alive.len() < 3 {
            continue;
        }
        let on_ring = roads.ring_of_node(NodeId(i as u32)).is_some();
        for sid in alive {
            let seg = roads.segment(sid);
            let (a, b) = roads.segment_points(sid);
            let (far, _) = if seg.a.0 as usize == i { (b, a) } else { (a, b) };
            let d = (far - node.pos).normalize_or_zero();
            let right = Vec2::new(-d.y, d.x);
            let hw = seg.half_width();
            if on_ring {
                // No crossing on the ring itself; arms get theirs set back
                // past a splitter island, and a give-way line at the mouth.
                if roads.ring_of_segment(sid).is_none() {
                    roundabout_arm(&mut g, pal, node.pos, d, r, seg, seg.a.0 as usize == i);
                }
                continue;
            }
            let c0 = node.pos + d * (r + 0.8);
            let c1 = node.pos + d * (r + 3.2);
            let mut s = -hw + SHOULDER + 0.4;
            while s < hw - SHOULDER - 0.2 {
                g.part(Glow).ground_strip(&[c0 + right * s, c1 + right * s], 0.5, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 0.9)));
                s += 1.0;
            }
            let incoming = if seg.a.0 as usize == i { seg.lanes_bwd } else { seg.lanes_fwd } as f32;
            if incoming > 0.0 {
                let w = incoming * LANE_WIDTH;
                let centre = node.pos + d * (r + 3.7) - right * (w * 0.5);
                g.part(Glow).ground_strip(&[centre - right * w * 0.5, centre + right * w * 0.5], 0.4, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 1.1)));
            }
        }
    }

    if let Some((from, z)) = node_from.take() {
        if z != 0.0 {
            lift_since(&mut g, &from, |_| z);
        }
    }

    // Signal posts: pole at the kerb, arm over the approach, one head per lane.
    for post in signal_groups(net) {
        let near_node = roads.nodes.iter().enumerate().min_by(|a, b| a.1.pos.distance_squared(post.pole).total_cmp(&b.1.pos.distance_squared(post.pole))).map(|(i, _)| NodeId(i as u32));
        let underground = near_node.is_some_and(|n| roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive).all(|&s| roads.segment(s).tunnel));
        if !inp.layer.keeps(underground) {
            continue;
        }
        let from = marks(&g);
        let base = post.pole;
        let pole_h = 5.6;
        g.part(Matte).cylinder(base, 0.16, 0.12, 0.0, pole_h, 8, rgba(CHARCOAL));
        let a = Vec3::new(base.x, pole_h - 0.2, base.y);
        let b = Vec3::new(post.leftmost.x, pole_h - 0.2, post.leftmost.y);
        g.part(Matte).beam(a, b, 0.18, rgba(CHARCOAL));
        for lamp in &post.heads {
            let head = Vec3::new(lamp.x, 4.75, lamp.y);
            g.part(Matte).boxed(head, Vec3::new(0.35, 1.1, 0.4), -post.heading, rgba(DARK));
            g.part(Matte).beam(Vec3::new(lamp.x, pole_h - 0.2, lamp.y), head + Vec3::Y * 0.55, 0.08, rgba(CHARCOAL));
        }
        if !heights.is_empty() {
            // At the height of the junction it stands at.
            let near = roads.nodes.iter().enumerate().min_by(|a, b| a.1.pos.distance_squared(base).total_cmp(&b.1.pos.distance_squared(base)));
            if let Some((i, _)) = near {
                let z = node_z(NodeId(i as u32));
                lift_since(&mut g, &from, |_| z);
            }
        }
    }
    g
}

/// The inner edge of a roundabout: a low, mountable cobbled apron on the
/// shoulder inside the ring lanes, for long vehicles to cut over. The
/// ground inside the ring is left open, as land like any other.
fn roundabout_apron(g: &mut Geometry, pal: &Palette, ring: &nd_sim::roads::Ring) {
    use std::f32::consts::TAU;
    let r = ring.radius - ring.lanes as f32 * LANE_WIDTH * 0.5 - SHOULDER * 0.5;
    let n = ((TAU * r / 3.0) as usize).clamp(24, 256);
    let mut edge: Vec<Vec2> = (0..n).map(|k| ring.center + Vec2::from_angle(k as f32 / n as f32 * TAU) * r).collect();
    edge.push(edge[0]);
    g.part(Paving).path_box(&edge, 0.0, SHOULDER - 0.2, 0.0, ASPHALT_Y + 0.06, rgba(mul(pal.paving, 0.85)));
}

/// Where an arm meets a roundabout: a splitter island down the middle of a
/// two-way arm, a zebra set back from the ring across it (with a refuge in
/// the island), and a dashed give-way line across the lanes going in.
fn roundabout_arm(g: &mut Geometry, pal: &Palette, at: Vec2, d: Vec2, r: f32, seg: &nd_sim::Segment, from_a: bool) {
    let right = Vec2::new(-d.y, d.x);
    let hw = seg.half_width();
    let (outgoing, incoming) = if from_a { (seg.lanes_fwd, seg.lanes_bwd) } else { (seg.lanes_bwd, seg.lanes_fwd) };
    let total = (outgoing + incoming) as f32;
    // The boundary between the directions, as a lateral offset (right of
    // `d` positive): lanes going out keep right, lanes coming in are left.
    let divide = (incoming as f32 - total * 0.5) * LANE_WIDTH;
    let (z0, z1) = (r + 5.0, r + 7.4);
    if outgoing > 0 && incoming > 0 {
        for (s0, s1, w0) in [(r + 0.9, z0 - 0.3, 0.5), (z1 + 0.3, r + 12.0, 0.9)] {
            let a = at + d * s0 + right * divide;
            let b = at + d * s1 + right * divide;
            g.part(Concrete).path_box(&[a, b], 0.0, w0, 0.0, KERB_H, rgba(pal.curb));
            g.part(Glow).path_strip(&[a - d * 0.6, b + d * 0.6], 0.0, w0 + 0.5, MARK_Y, rgba(mul(pal.lane_marker, 0.5)));
        }
    }
    let mut s = -hw + SHOULDER + 0.4;
    while s < hw - SHOULDER - 0.2 {
        if (s - divide).abs() > 0.6 || outgoing == 0 || incoming == 0 {
            g.part(Glow).ground_strip(&[at + d * z0 + right * s, at + d * z1 + right * s], 0.5, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 0.9)));
        }
        s += 1.0;
    }
    if incoming > 0 {
        let (l0, l1) = (-(hw - SHOULDER), divide);
        let mut x = l0 + 0.3;
        while x + 0.6 < l1 {
            let p = at + d * (r + 0.9) + right * x;
            g.part(Glow).ground_strip(&[p, p + right * 0.6], 0.35, MARK_Y + 0.003, rgba(mul(pal.lane_marker, 1.1)));
            x += 1.1;
        }
    }
}

/// Neglect on a cheap street: potholes as dark patches on the carriageway,
/// cracks across the pavement slabs and litter blown up against the kerb.
/// The strips themselves are untouched; this only lays thin things on top.
fn street_grit(g: &mut Geometry, pal: &Palette, walk: &[Vec2], hw: f32, len: f32, rng: &mut impl Rng) {
    let pothole = rgba(mul(pal.asphalt, 0.5));
    let crack = rgba(mul(pal.paving, 0.35));
    let at = |s: f32| {
        let (p, d) = point_along(walk, s);
        (p, d, Vec2::new(-d.y, d.x))
    };
    // Potholes.
    let lane_room = hw - SHOULDER - 0.7;
    if lane_room > 0.2 {
        let mut s = rng.random_range(3.0..12.0);
        while s < len - 2.0 {
            let (p, _, right) = at(s);
            let c = p + right * rng.random_range(-lane_room..lane_room);
            let r = rng.random_range(0.3..0.8);
            let rot = rng.random_range(0.0..std::f32::consts::TAU);
            let squash = rng.random_range(0.6..1.0);
            let poly: Vec<Vec2> = (0..7)
                .map(|k| {
                    let a = k as f32 / 7.0 * std::f32::consts::TAU;
                    let (x, y) = (a.cos() * r, a.sin() * r * squash);
                    c + Vec2::new(x * rot.cos() - y * rot.sin(), x * rot.sin() + y * rot.cos())
                })
                .collect();
            g.part(Asphalt).ground_polygon(&poly, ASPHALT_Y + 0.006, pothole);
            s += rng.random_range(9.0..22.0);
        }
    }
    // Cracks in the pavement slabs, with a branch now and then.
    let mut s = 2.0;
    while s < len - 1.0 {
        for side in [-1.0f32, 1.0] {
            if !rng.random_bool(0.55) {
                continue;
            }
            let (p, d, right) = at(s);
            let yaw = d.y.atan2(d.x) + rng.random_range(-0.6..0.6);
            let c = p + right * side * (hw + 0.3 + rng.random_range(0.4..SIDEWALK - 0.8));
            let l = rng.random_range(0.6..2.2);
            g.part(Matte).boxed(Vec3::new(c.x, KERB_H - 0.005, c.y), Vec3::new(l, 0.012, 0.05), yaw, crack);
            if rng.random_bool(0.5) {
                let b = c + Vec2::new(yaw.cos(), yaw.sin()) * l * 0.3;
                g.part(Matte).boxed(Vec3::new(b.x, KERB_H - 0.005, b.y), Vec3::new(l * 0.5, 0.012, 0.05), yaw + rng.random_range(0.6..1.2), crack);
            }
        }
        s += rng.random_range(4.0..9.0);
    }
    // Litter against the kerb: bags, cans, sheets of plastic, cardboard.
    let mut s = 1.0;
    while s < len - 1.0 {
        let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let (p, d, right) = at(s);
        let c = p + right * side * (hw + rng.random_range(0.35..1.1));
        let yaw = d.y.atan2(d.x) + rng.random_range(0.0..std::f32::consts::TAU);
        let base = KERB_H - 0.01;
        match rng.random_range(0..4) {
            0 => g.part(Tarp).boxed(Vec3::new(c.x, base + 0.14, c.y), Vec3::new(0.4, 0.28, 0.32), yaw, rgba([0.07, 0.07, 0.08])),
            1 => g.part(Matte).boxed(Vec3::new(c.x, base + 0.06, c.y), Vec3::new(0.1, 0.12, 0.1), yaw, rgba(mul(pal.paving, 1.4))),
            2 => g.part(Tarp).boxed(Vec3::new(c.x, base + 0.01, c.y), Vec3::new(0.5, 0.02, 0.4), yaw, rgba(mul(pick(rng, &pal.tarp_colors), 0.8))),
            _ => g.part(Wood).boxed(Vec3::new(c.x, base + 0.025, c.y), Vec3::new(0.6, 0.05, 0.45), yaw, rgba(mul(pal.plywood, 0.75))),
        }
        s += rng.random_range(5.0..14.0);
    }
}

/// Single track: ballast, sleepers, two rails, drawn by the same hand as a
/// freight yard's sidings.
fn railway(g: &mut Geometry, pal: &Palette, run: &Run) {
    railway_track(g, pal, &run.points, RAIL_HALF_WIDTH * 2.0);
}

/// Where track crosses a street: planks between the rails over the
/// carriageway, crossbuck signs and barrier posts on both sides.
fn level_crossing(g: &mut Geometry, pal: &Palette, roads: &RoadNetwork, at: Vec2, streets: &[SegmentId], rails: &[SegmentId]) {
    let Some(&street) = streets.first() else { return };
    let Some(&rail) = rails.first() else { return };
    let (sa, sb) = roads.segment_points(street);
    let sd = (sb - sa).normalize_or_zero();
    let s_right = Vec2::new(-sd.y, sd.x);
    let (ra, rb) = roads.segment_points(rail);
    let rd = (ra - rb).normalize_or_zero();
    let hw = roads.segment(street).half_width();
    // Planks either side of each rail, the length of the carriageway.
    for off in [-1.2f32, 0.0, 1.2] {
        let a = at + rd * (hw + 0.5) + Vec2::new(-rd.y, rd.x) * off;
        let b = at - rd * (hw + 0.5) + Vec2::new(-rd.y, rd.x) * off;
        g.part(Wood).ground_strip(&[a, b], 0.9, 0.2, rgba(mul(pal.wood, 0.5)));
    }
    // Signs and posts where the pavement meets the track.
    for side in [-1.0f32, 1.0] {
        for s2 in [-1.0f32, 1.0] {
            let p = at + sd * side * (RAIL_HALF_WIDTH + 1.6) + s_right * s2 * (hw + SIDEWALK * 0.4);
            g.part(Matte).boxed(Vec3::new(p.x, 1.6, p.y), Vec3::new(0.1, 3.2, 0.1), 0.0, rgba(mul(pal.steel, 0.6)));
            let yaw = sd.y.atan2(sd.x);
            g.part(Matte).boxed(Vec3::new(p.x, 3.0, p.y), Vec3::new(1.2, 0.18, 0.04), yaw + 0.6, rgba(pal.chalk));
            g.part(Matte).boxed(Vec3::new(p.x, 3.0, p.y), Vec3::new(1.2, 0.18, 0.04), yaw - 0.6, rgba(pal.chalk));
            g.part(Glow).boxed(Vec3::new(p.x, 2.4, p.y), Vec3::new(0.22, 0.22, 0.1), yaw, rgba(pal.warning_light));
            // Barrier arm, raised.
            g.part(Matte).boxed(Vec3::new(p.x, 2.2, p.y), Vec3::new(0.12, 2.8, 0.12), 0.0, rgba(pal.chalk));
            g.part(Matte).boxed(Vec3::new(p.x, 2.6, p.y), Vec3::new(0.14, 0.4, 0.14), 0.0, rgba(mul(pal.warning_light, 0.4)));
        }
    }
}

/// A path for walkers and bikes: paving with edge stones, low lamps, the odd
/// bench and bin, and bollards where it meets a street so cars stay out.
fn footpath(g: &mut Geometry, pal: &Palette, roads: &RoadNetwork, run: &Run, rng: &mut impl Rng) {
    // Where the path meets a street, stop at the back of that street's
    // pavement and flare out onto it, instead of running under the kerb.
    let mut points = run.points.clone();
    for (end, node) in [(true, run.start_node), (false, run.end_node)] {
        if roads.street_degree(node) == 0 || points.len() < 2 {
            continue;
        }
        let widest = roads.node(node).segments.iter().filter(|&&s| roads.segment(s).alive && roads.segment(s).is_street()).map(|&s| roads.segment(s).half_width()).fold(0.0, f32::max);
        let back = widest + SIDEWALK;
        let (a, b) = if end { (points[1], points[0]) } else { (points[points.len() - 2], points[points.len() - 1]) };
        let len = a.distance(b);
        if len <= back + 1.0 {
            continue;
        }
        let d = (b - a) / len;
        let stop = b - d * back;
        if end {
            points[0] = stop;
        } else {
            let n = points.len();
            points[n - 1] = stop;
        }
        // A small apron onto the pavement.
        let right = Vec2::new(-d.y, d.x);
        let apron = [stop + right * (PATH_HALF_WIDTH + 1.2), stop + right * (PATH_HALF_WIDTH + 1.2) + d * 1.6, stop - right * (PATH_HALF_WIDTH + 1.2) + d * 1.6, stop - right * (PATH_HALF_WIDTH + 1.2)];
        g.part(Paving).ground_polygon(&apron, ASPHALT_Y + 0.02, rgba(mul(pal.paving, 1.05)));
    }
    let pts = &points;
    let w = PATH_HALF_WIDTH * 2.0;
    g.part(Paving).path_strip(pts, 0.0, w, ASPHALT_Y + 0.02, rgba(mul(pal.paving, 1.05)));
    g.part(Concrete).path_box(pts, PATH_HALF_WIDTH + 0.1, 0.2, 0.0, 0.08, rgba(pal.curb));
    g.part(Concrete).path_box(pts, -(PATH_HALF_WIDTH + 0.1), 0.2, 0.0, 0.08, rgba(pal.curb));
    // Centre line for bikes to keep to one side.
    g.part(Glow).path_strip(pts, 0.0, 0.08, MARK_Y, rgba(mul(pal.lane_divider, 0.6)));
    let len = path_length(pts);
    let at = |s: f32, off: f32| {
        let (p, d) = point_along(pts, s);
        let right = Vec2::new(-d.y, d.x);
        (p + right * off, d)
    };
    let mut s = 8.0;
    let mut side = 1.0;
    while s < len - 4.0 {
        let (p, _) = at(s, side * (PATH_HALF_WIDTH + 0.6));
        lamp_post_world(g, p, 4.0, mul(pal.lamp, 0.9));
        s += 22.0;
        side = -side;
    }
    let mut s = 14.0 + rng.random_range(0.0..10.0);
    while s < len - 6.0 {
        let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let (p, d) = at(s, side * (PATH_HALF_WIDTH + 0.9));
        if rng.random_bool(0.6) {
            bench_world(g, p, d.y.atan2(d.x) + if side > 0.0 { std::f32::consts::PI } else { 0.0 }, pal.wood, CHARCOAL);
        } else {
            bin_world(g, p, mul(pal.metal_paint(0), 0.9));
        }
        s += 26.0 + rng.random_range(0.0..20.0);
    }
    // Bollards where the path meets a street.
    for (s_end, node) in [(0.0, run.start_node), (len, run.end_node)] {
        if roads.street_degree(node) == 0 {
            continue;
        }
        let back = if s_end == 0.0 { 1.0 } else { -1.0 };
        let (p, d) = point_along(pts, (s_end + back * 2.0).clamp(0.0, len));
        let right = Vec2::new(-d.y, d.x);
        for off in [-0.9f32, 0.0, 0.9] {
            bollard_world(g, p + right * off, mul(pal.steel, 0.7));
        }
    }
}

/// Bus shelter: two posts, a roof, a back panel and a lit advert.
pub fn shelter(g: &mut Geometry, pal: &Palette, p: Vec2, d: Vec2, out: Vec2) {
    let yaw = d.y.atan2(d.x);
    for s in [-1.6f32, 1.6] {
        let q = p + d * s;
        g.part(Matte).boxed(Vec3::new(q.x, 1.3, q.y), Vec3::new(0.1, 2.6, 0.1), yaw, rgba(CHARCOAL));
    }
    g.part(Panel).boxed(Vec3::new(p.x, 2.65, p.y), Vec3::new(3.8, 0.12, 1.8), yaw, rgba(mul(pal.steel, 0.9)));
    let back = p + out * 0.8;
    g.part(Glass).boxed(Vec3::new(back.x, 1.4, back.y), Vec3::new(3.6, 2.2, 0.05), yaw, rgba(pal.glass_dark));
    let ad = p + out * 0.78 + d * 1.0;
    g.part(Glow).decal(Vec3::new(ad.x, 1.5, ad.y), Vec3::new(d.x, 0.0, d.y), Vec3::Y, 1.2, 1.6, Vec3::new(-out.x, 0.0, -out.y), rgba(mul(pal.led(2), 0.45)));
    g.part(Glow).boxed(Vec3::new(p.x, 2.57, p.y), Vec3::new(2.0, 0.05, 0.4), yaw, rgba(mul(pal.lamp, 0.6)));
    let seat = p + out * 0.4;
    g.part(Wood).boxed(Vec3::new(seat.x, 0.5, seat.y), Vec3::new(2.4, 0.08, 0.4), yaw, rgba(pal.wood));
}

struct SignalGroup {
    pole: Vec2,
    leftmost: Vec2,
    heads: Vec<Vec2>,
    heading: f32,
}

/// Road lanes feeding each connector lane, built once: the signal passes
/// used to scan every lane for every connector of every junction, and the
/// result came out of a `HashMap` in a different order each run.
fn feeders(net: &LaneNetwork) -> BTreeMap<LaneId, Vec<LaneId>> {
    let mut map: BTreeMap<LaneId, Vec<LaneId>> = BTreeMap::new();
    for (lid, lane) in net.lanes.iter().enumerate() {
        if lane.is_connector() {
            continue;
        }
        for &next in &lane.next {
            map.entry(next).or_default().push(lid as LaneId);
        }
    }
    map
}

/// Approaches to signalised junctions grouped by segment and direction, in
/// segment order so the street mesh is the same every time it is built.
fn signal_groups(net: &LaneNetwork) -> Vec<SignalGroup> {
    let feeders = feeders(net);
    let mut groups: BTreeMap<(u32, bool), Vec<(u8, Vec2, Vec2)>> = BTreeMap::new();
    for inter in &net.intersections {
        if !inter.signalized {
            continue;
        }
        for &(conn, _) in &inter.connectors {
            for &lid in feeders.get(&conn).map(|v| v.as_slice()).unwrap_or(&[]) {
                let lane = &net.lanes[lid as usize];
                if let LaneKind::Road { segment, forward, index } = lane.kind {
                    let (p, d) = lane.point_at(lane.length - 0.6);
                    let entry = groups.entry((segment.0, forward)).or_default();
                    if !entry.iter().any(|(i, _, _)| *i == index) {
                        entry.push((index, p, d));
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for lanes in groups.into_values() {
        let Some((_, _, d)) = lanes.first() else { continue };
        let d = *d;
        let right = Vec2::new(-d.y, d.x);
        let rightmost = lanes.iter().max_by_key(|(i, _, _)| *i).unwrap();
        let leftmost = lanes.iter().min_by_key(|(i, _, _)| *i).unwrap();
        let pole = rightmost.1 + right * (LANE_WIDTH * 0.5 + SHOULDER + 0.45);
        out.push(SignalGroup { pole, leftmost: leftmost.1 - right * (LANE_WIDTH * 0.2), heads: lanes.iter().map(|(_, p, _)| *p).collect(), heading: d.y.atan2(d.x) });
    }
    out
}

/// Lamp positions for the renderer, one per lane feeding a signal, in lane
/// order.
pub fn signal_posts(net: &LaneNetwork) -> Vec<SignalPost> {
    let feeders = feeders(net);
    let mut seen = HashSet::new();
    let mut lanes: Vec<LaneId> = Vec::new();
    for inter in &net.intersections {
        if !inter.signalized {
            continue;
        }
        for &(conn, _) in &inter.connectors {
            for &lid in feeders.get(&conn).map(|v| v.as_slice()).unwrap_or(&[]) {
                if seen.insert(lid) {
                    lanes.push(lid);
                }
            }
        }
    }
    lanes.sort_unstable();
    lanes
        .into_iter()
        .map(|lid| {
            let lane = &net.lanes[lid as usize];
            let (p, d) = lane.point_at(lane.length - 0.6);
            SignalPost { lane: lid, lamp: Vec3::new(p.x - d.x * 0.24, 4.95, p.y - d.y * 0.24), heading: d.y.atan2(d.x) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::roads::RoadNetwork;

    /// The street network the building sweep uses: straight, diagonal and
    /// curved roads crossing, so there are bends, junctions and signals.
    fn network() -> (RoadNetwork, LaneNetwork) {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-400.0, -40.0), Vec2::new(400.0, -40.0)], 2, 2);
        roads.add_polyline(&[Vec2::new(-120.0, -300.0), Vec2::new(60.0, 300.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(-400.0, 200.0), Vec2::new(400.0, 200.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 2, 2);
        let curve: Vec<Vec2> = (0..=20).map(|i| {
            let t = i as f32 / 20.0;
            Vec2::new(-300.0 + 600.0 * t, 80.0 + 60.0 * (t * std::f32::consts::TAU).sin())
        }).collect();
        roads.add_polyline(&curve, 1, 1);
        roads.add_polyline_kind(&[Vec2::new(-350.0, 150.0), Vec2::new(350.0, 150.0)], 1, 1, RoadKind::Rail);
        roads.add_polyline_kind(&[Vec2::new(-200.0, -40.0), Vec2::new(-200.0, 200.0)], 1, 1, RoadKind::Path);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        (roads, net)
    }

    #[test]
    fn one_way_roads_show_their_direction_and_ramps_get_a_gore() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 3, 0);
        roads.add_polyline(&[Vec2::new(-100.0, -150.0), Vec2::new(-50.0, -12.0), Vec2::new(0.0, 0.0)], 1, 0);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let lv = LandValue::new(400.0, 50.0);
        let pal = Palette::default();
        let g = generate_street(&StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 3, heights: &[], layer: StreetLayer::All });
        for (_, m) in g.iter() {
            assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())));
        }
        // Arrows in the highway's lanes: glow strips pointing +x somewhere
        // mid-carriageway, far from the join.
        let glow = g.get(Glow);
        assert!(glow.positions.iter().any(|p| p[0] > 100.0 && p[0] < 300.0 && p[2].abs() < 5.0 && (p[1] - MARK_Y - 0.003).abs() < 1e-3), "arrows on the highway");
        // A gore: hatching between the ramp and the carriageway near the join.
        assert!(glow.positions.iter().any(|p| p[0] < -2.0 && p[0] > -45.0 && p[2] < -5.0 && (p[1] - MARK_Y - 0.004).abs() < 1e-3), "chevrons in the gore");
        // No round junction pad at the join.
        let pad_y = ASPHALT_Y + 0.01;
        assert!(!g.get(Asphalt).positions.iter().any(|p| (p[1] - pad_y).abs() < 1e-4 && Vec2::new(p[0], p[2]).length() < 15.0), "no pad over the merge");
    }

    #[test]
    fn streets_are_finite_low_and_deterministic() {
        let (roads, net) = network();
        let lv = LandValue::new(400.0, 50.0);
        let pal = Palette::default();
        let inp = StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 5, heights: &[], layer: StreetLayer::All };
        let a = generate_street(&inp);
        assert!(a.triangle_count() > 1000, "{} triangles is not a street network", a.triangle_count());
        for (s, m) in a.iter() {
            assert_eq!(m.positions.len(), m.uvs.len(), "{s:?} uv count");
            assert_eq!(m.positions.len(), m.tangents.len(), "{s:?} tangent count");
            for p in &m.positions {
                assert!(p.iter().all(|v| v.is_finite()), "{s:?} has NaN");
                assert!(p[1] >= -0.01 && p[1] < 12.0, "{s:?} at height {}", p[1]);
            }
            assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()), "{s:?} bad index");
        }
        let b = generate_street(&inp);
        assert!(a == b, "the same network drew two different streets");
        // Land value zero everywhere: the runs are drawn worn. Well-off: clean.
        assert!(!a.get(WornAsphalt).is_empty() && !a.get(WornPaving).is_empty(), "cheap streets use the worn materials");
        let mut rich = LandValue::new(400.0, 50.0);
        for v in &mut rich.values {
            *v = 0.9;
        }
        let r = generate_street(&StreetInput { land_value: &rich, ..inp });
        assert!(r.get(WornAsphalt).is_empty() && r.get(WornPaving).is_empty(), "smart streets stay clean");
        let posts = signal_posts(&net);
        assert!(!posts.is_empty(), "the four-way junctions should be signalised");
        assert!(posts.windows(2).all(|w| w[0].lane < w[1].lane), "signal posts come in lane order");
        assert!(posts.iter().all(|p| p.lamp.is_finite()));
    }

    #[test]
    fn a_roundabout_leaves_its_middle_open_and_has_no_signals() {
        use nd_sim::plan::{Gesture, GestureId, Plan};
        let mut plan = Plan::default();
        plan.gestures.insert(GestureId(1), Gesture::Road { points: vec![Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        plan.gestures.insert(GestureId(2), Gesture::Road { points: vec![Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], lanes_fwd: 2, lanes_bwd: 2, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        let lv = LandValue::new(400.0, 50.0);
        let pal = Palette::default();
        let mut tris = Vec::new();
        for g in [None, Some(Gesture::Roundabout { center: Vec2::ZERO, radius: 30.0, lanes: 2 })] {
            let mut plan = plan.clone();
            if let Some(g) = g {
                plan.gestures.insert(GestureId(3), g);
            }
            let (built, rings) = plan.built_roads();
            let mut roads = RoadNetwork::default();
            roads.rings = rings;
            for (_, pts, f, b, k) in built {
                roads.add_polyline_kind(&pts, f, b, k);
            }
            let mut net = LaneNetwork::default();
            net.rebuild(&roads);
            let geo = generate_street(&StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 9, heights: &[], layer: StreetLayer::All });
            for (s, m) in geo.iter() {
                assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite()) && p[1] < 20.0), "{s:?}");
            }
            tris.push((geo.get(Foliage).indices.len(), geo.get(Stone).indices.len(), signal_posts(&net).len()));
        }
        let (plain, ring) = (tris[0], tris[1]);
        assert!(plain.2 > 0 && ring.2 == 0, "the signals went with the junction: {plain:?} {ring:?}");
        assert_eq!(ring.1, plain.1, "no sculpture: the middle is open land");
    }
}

/// Lane arrows painted on each lane arriving at a junction, a few metres
/// before the stop line: a shaft with a head for every movement the lane
/// allows (`Lane::arrows`). Separate from the street mesh because arrows
/// follow turning demand and the player's choices without the roads
/// changing; rebuild when `LaneNetwork::arrows_version` moves.
pub fn lane_arrows(net: &LaneNetwork, pal: &Palette) -> Geometry {
    use nd_sim::traffic::{LEFT, RIGHT, STRAIGHT};
    let mut g = Geometry::default();
    let c = rgba(mul(pal.lane_marker, 1.1));
    let y = MARK_Y + 0.003;
    for ap in &net.approaches {
        for (&l, &a) in ap.lanes.iter().zip(&ap.arrows) {
            let lane = &net.lanes[l as usize];
            if lane.length < 16.0 || a == 0 {
                continue;
            }
            let (fork, d) = lane.point_at(lane.length - 9.0);
            let right = Vec2::new(-d.y, d.x);
            g.part(Glow).ground_strip(&[fork - d * 1.6, fork], 0.22, y, c);
            for (bit, side) in [(LEFT, -1.0f32), (STRAIGHT, 0.0), (RIGHT, 1.0)] {
                if a & bit == 0 {
                    continue;
                }
                let tip = if side == 0.0 { fork + d * 1.4 } else { fork + d * 0.7 + right * side * 0.75 };
                g.part(Glow).ground_strip(&[fork, tip], 0.22, y, c);
                let dir = (tip - fork).normalize_or_zero();
                let across = Vec2::new(-dir.y, dir.x);
                for s in [-1.0f32, 1.0] {
                    g.part(Glow).ground_strip(&[tip, tip - dir * 0.55 + across * s * 0.35], 0.2, y, c);
                }
            }
        }
    }
    g
}
