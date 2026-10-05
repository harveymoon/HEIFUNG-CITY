//! Streets: carriageway, kerbs and pavements, markings and crossings,
//! junction pads, lamps, signal posts, bins, benches, shelters, bollards,
//! utility poles with sagging wires, and trees where the land is worth it.
//!
//! Roads are drawn as *runs*: chains of segments joined at plain bends
//! (nodes with exactly two roads of the same kind). A run gets one
//! continuous, mitred carriageway and pavement, so a curved road reads as
//! one road instead of a string of pieces.

use crate::buildings::parts::{bench_world, bin_world, bollard_world, lamp_post_world, mul, rgba, tree_world, CHARCOAL, DARK};
use crate::mesh::{offset_path, path_length, point_along};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::land_value::LandValue;
use nd_sim::rng_for;
use nd_sim::roads::{RoadKind, RoadNetwork, SegmentId, LANE_WIDTH, PATH_HALF_WIDTH, SHOULDER};
use nd_sim::traffic::{LaneId, LaneKind, LaneNetwork};
use nd_sim::zoning::SIDEWALK;
use nd_sim::NodeId;
use rand::Rng;
use std::collections::HashSet;

pub struct StreetInput<'a> {
    pub roads: &'a RoadNetwork,
    pub net: &'a LaneNetwork,
    pub land_value: &'a LandValue,
    pub palette: &'a Palette,
    pub seed: u64,
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
    (f == lanes_fwd && b == lanes_bwd).then_some(next)
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
        let mut node = start_node;
        let mut cur = start_seg;
        let (_, run_f, run_b) = oriented(roads, cur, node);
        let first = cur;
        loop {
            used.insert(cur.0);
            let (far, _, _) = oriented(roads, cur, node);
            points.push(roads.node(far).pos);
            node = far;
            match continuation(roads, node, cur, run_f, run_b) {
                Some(next) if !used.contains(&next.0) => cur = next,
                _ => break,
            }
        }
        runs.push(Run { points, first, lanes_fwd: run_f, lanes_bwd: run_b, kind: roads.segment(first).kind, start_node, end_node: node });
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

    for run in build_runs(roads) {
        let seg = roads.segment(run.first);
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
        // Carriageway and markings run the whole way; the junction pads overlap the ends.
        g.part(Asphalt).path_strip(pts, 0.0, hw * 2.0, ASPHALT_Y, asphalt_c);
        g.part(Glow).path_strip(pts, hw - SHOULDER * 0.5, 0.18, MARK_Y, edge_c);
        g.part(Glow).path_strip(pts, -(hw - SHOULDER * 0.5), 0.18, MARK_Y, edge_c);
        for k in 1..(run.lanes_fwd + run.lanes_bwd) as u32 {
            let off = (k as f32 - total * 0.5) * LANE_WIDTH;
            let is_centre = run.lanes_fwd > 0 && run.lanes_bwd > 0 && k == run.lanes_bwd as u32;
            if is_centre {
                g.part(Glow).path_strip(pts, off + 0.18, 0.14, MARK_Y, centre_c);
                g.part(Glow).path_strip(pts, off - 0.18, 0.14, MARK_Y, centre_c);
            } else {
                let path = offset_path(pts, off);
                dashes_along(g.part(Glow), &path, 0.16, MARK_Y, 3.0, 4.0, divider_c);
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

        // Furniture by arc length along the pavement.
        let len = path_length(&walk);
        let lv_mid = inp.land_value.at(point_along(&walk, len * 0.5).0);
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
                0 | 1 => bin_world(&mut g, p, mul(pal.metal_paint[0], 0.9)),
                2 if lv_mid > 0.35 => {
                    let (bp, _, _) = at(s, side, back_off);
                    bench_world(&mut g, bp, d.y.atan2(d.x) + if side > 0.0 { std::f32::consts::PI } else { 0.0 }, pal.wood, CHARCOAL);
                }
                3 if lv_mid < 0.55 => {
                    let (kp, _, _) = at(s, side, hw + 1.4);
                    let yaw = d.y.atan2(d.x);
                    g.part(Metal).boxed(Vec3::new(kp.x, 1.2, kp.y), Vec3::new(1.0, 2.4, 0.9), yaw, rgba(pal.metal_paint[2]));
                    let face = kp - right * side * 0.47;
                    g.part(Glow).boxed(Vec3::new(face.x, 1.5, face.y), Vec3::new(0.6, 0.8, 0.04), yaw, rgba(mul(pal.led_colors[1], 0.5)));
                }
                4 if len > 80.0 => {
                    let (sp, d, right) = at(s, side, hw + SIDEWALK * 0.55);
                    shelter(&mut g, pal, sp, d, right * side);
                }
                _ => {}
            }
            s += 22.0 + rng.random_range(0.0..20.0);
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

    // Junction pads, crossings and stop lines (bends need none).
    for (i, node) in roads.nodes.iter().enumerate() {
        let alive: Vec<_> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).count_and_list();
        let paths_here = node.segments.iter().filter(|&&s| roads.segment(s).alive && !roads.segment(s).is_street()).count();
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
        for sid in alive {
            let seg = roads.segment(sid);
            let (a, b) = roads.segment_points(sid);
            let (far, _) = if seg.a.0 as usize == i { (b, a) } else { (a, b) };
            let d = (far - node.pos).normalize_or_zero();
            let right = Vec2::new(-d.y, d.x);
            let hw = seg.half_width();
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

    // Signal posts: pole at the kerb, arm over the approach, one head per lane.
    for post in signal_groups(net) {
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
    }
    g
}

trait CountAndList: Iterator<Item = SegmentId> + Sized {
    fn count_and_list(self) -> Vec<SegmentId> {
        self.collect()
    }
}
impl<I: Iterator<Item = SegmentId>> CountAndList for I {}

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
            bin_world(g, p, mul(pal.metal_paint[0], 0.9));
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
fn shelter(g: &mut Geometry, pal: &Palette, p: Vec2, d: Vec2, out: Vec2) {
    let yaw = d.y.atan2(d.x);
    for s in [-1.6f32, 1.6] {
        let q = p + d * s;
        g.part(Matte).boxed(Vec3::new(q.x, 1.3, q.y), Vec3::new(0.1, 2.6, 0.1), yaw, rgba(CHARCOAL));
    }
    g.part(Panel).boxed(Vec3::new(p.x, 2.65, p.y), Vec3::new(3.8, 0.12, 1.8), yaw, rgba(mul(pal.steel, 0.9)));
    let back = p + out * 0.8;
    g.part(Glass).boxed(Vec3::new(back.x, 1.4, back.y), Vec3::new(3.6, 2.2, 0.05), yaw, rgba(pal.glass_dark));
    let ad = p + out * 0.78 + d * 1.0;
    g.part(Glow).boxed(Vec3::new(ad.x, 1.5, ad.y), Vec3::new(1.2, 1.6, 0.04), yaw, rgba(mul(pal.led_colors[2], 0.45)));
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

/// Approaches to signalised junctions grouped by segment and direction.
fn signal_groups(net: &LaneNetwork) -> Vec<SignalGroup> {
    use std::collections::HashMap;
    let mut groups: HashMap<(u32, bool), Vec<(u8, Vec2, Vec2)>> = HashMap::new();
    for inter in &net.intersections {
        if !inter.signalized {
            continue;
        }
        for &(conn, _) in &inter.connectors {
            for lane in net.lanes.iter() {
                if !lane.next.contains(&conn) || lane.is_connector() {
                    continue;
                }
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
    for (_, lanes) in groups {
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

/// Lamp positions for the renderer, one per lane feeding a signal.
pub fn signal_posts(net: &LaneNetwork) -> Vec<SignalPost> {
    let mut out = Vec::new();
    for inter in &net.intersections {
        if !inter.signalized {
            continue;
        }
        let mut seen = std::collections::HashSet::new();
        for &(conn, _) in &inter.connectors {
            for (lid, lane) in net.lanes.iter().enumerate() {
                if lane.next.contains(&conn) && !lane.is_connector() && seen.insert(lid) {
                    let (p, d) = lane.point_at(lane.length - 0.6);
                    out.push(SignalPost { lane: lid as LaneId, lamp: Vec3::new(p.x - d.x * 0.24, 4.95, p.y - d.y * 0.24), heading: d.y.atan2(d.x) });
                }
            }
        }
    }
    out
}
