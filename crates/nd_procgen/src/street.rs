//! Streets: carriageway, kerbs and pavements, markings and crossings,
//! junction pads, lamps, signal posts, bins, benches, shelters, bollards,
//! utility poles with sagging wires, and trees where the land is worth it.
//!
//! Roads are drawn as *runs*: chains of segments joined at plain bends
//! (nodes with exactly two roads of the same kind). A run gets one
//! continuous, mitred carriageway and pavement, so a curved road reads as
//! one road instead of a string of pieces.

use crate::buildings::parts::{bench_world, bin_world, bollard_world, lamp_post_world, mul, pick, railway_track, rgba, CHARCOAL, DARK};
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
use std::collections::{BTreeMap, HashMap, HashSet};

#[path = "street_junction.rs"]
mod junction;
use junction::{cul_de_sac, gore_ramp, mouths, pad_node, Mouth};

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
    /// Heights inside long streets between their nodes, per segment
    /// (`World::road_inner`); empty for straight lines between the nodes.
    pub inner: &'a [Vec<(f32, f32)>],
    /// Which roads to draw: all, those on and above the ground, or only the
    /// tunnels (shown on their own in the underground view).
    pub layer: StreetLayer,
    /// [footbridge] Middles of the crosswalks a footbridge replaced
    /// (`World::closed_crosswalks`): no zebra, beacons or walk lights there.
    pub closed: &'a [Vec2],
}

/// [footbridge] Is the crosswalk with its middle at `mid` one a footbridge
/// replaced?
pub fn crosswalk_closed(closed: &[Vec2], mid: Vec2) -> bool {
    closed.iter().any(|c| c.distance(mid) < 2.0)
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

/// [pedestrians] A walk / don't walk lamp at one end of a signalised
/// crosswalk, facing the people at the other end: the renderer colours it
/// from `Traffic::peds` (`walk_shown(inter, approach)`).
pub struct PedSignalPost {
    pub inter: usize,
    pub approach: usize,
    /// [footbridge] The middle of the crosswalk it serves.
    pub crosswalk: Vec2,
    /// Foot of the pole, at the kerb.
    pub pole: Vec2,
    pub lamp: Vec3,
    /// Direction the lamp faces (over the street), as an angle.
    pub heading: f32,
}

/// Height of a pedestrian signal head.
const PED_HEAD_Y: f32 = 2.55;

/// How far a tunnel's walls stand outside its carriageway (half width).
pub const TUNNEL_WALL: f32 = 0.8;

const KERB_H: f32 = 0.16;
const ASPHALT_Y: f32 = 0.05;
const MARK_Y: f32 = 0.085;

/// Spring line of a tunnel bore (`bore_shell`): the walls stand straight
/// up to here, the vault springs from it.
pub const BORE_SPRING: f32 = 2.6;
/// Lining bands round the inside of a bore, this far apart along it.
const BORE_RIB_EVERY: f32 = 14.0;

/// The cross-section of a tunnel bore of carriageway half width `hw`, from
/// the right foot up the wall, over the vault and down the left wall:
/// (offset right of travel, height). A horseshoe: straight walls
/// `TUNNEL_WALL` outside the carriageway to [`BORE_SPRING`], then a half
/// ellipse whose rise grows with the width (a wide bore is not a slot).
pub fn bore_profile(hw: f32) -> Vec<(f32, f32)> {
    let r = hw + TUNNEL_WALL;
    let rise = (r * 0.75).clamp(2.6, 4.6);
    let n = 12;
    let mut v = vec![(r, 0.0), (r, BORE_SPRING)];
    for k in 1..n {
        let a = std::f32::consts::PI * k as f32 / n as f32;
        v.push((r * a.cos(), BORE_SPRING + rise * a.sin()));
    }
    v.push((-r, BORE_SPRING));
    v.push((-r, 0.0));
    v
}

/// The direction into the bore from a point (x, y) of its cross-section:
/// straight in across the walls, towards the spring line's centre in the vault.
fn bore_inward(x: f32, y: f32) -> Vec2 {
    Vec2::new(-x, if y > BORE_SPRING { BORE_SPRING - y } else { 0.0 }).normalize_or_zero()
}

/// A tunnel bore as a tube along `walk`: a concrete lining, darker lining
/// bands every [`BORE_RIB_EVERY`] metres and a lamp line along each wall.
///
/// Every face looks *into* the tube and the materials cull back faces, so
/// whichever half of the tube is nearer the camera (its outside towards the
/// viewer) is never drawn: the near side is cut away from any angle and the
/// cars inside show, with the far wall and vault behind them. No per-frame
/// work: the cut comes from the winding alone.
pub fn bore_shell(g: &mut Geometry, walk: &[Vec2], hw: f32, lining: [f32; 4], rib: [f32; 4], lamp: [f32; 4]) {
    if walk.len() < 2 {
        return;
    }
    let profile = bore_profile(hw);
    let lines: Vec<Vec<Vec2>> = profile.iter().map(|&(x, _)| offset_path(walk, x)).collect();
    // A point of the section (profile index k) at walk point i.
    let at = |k: usize, i: usize| {
        let q = lines[k][i];
        Vec3::new(q.x, profile[k].1, q.y)
    };
    let inward3 = |right: Vec2, x: f32, y: f32| {
        let d = bore_inward(x, y);
        Vec3::new(right.x * d.x, d.y, right.y * d.x)
    };
    for i in 0..walk.len() - 1 {
        let d = (walk[i + 1] - walk[i]).normalize_or_zero();
        let right = Vec2::new(-d.y, d.x);
        for k in 0..profile.len() - 1 {
            let (mx, my) = ((profile[k].0 + profile[k + 1].0) * 0.5, (profile[k].1 + profile[k + 1].1) * 0.5);
            g.part(Concrete).quad_facing([at(k, i), at(k, i + 1), at(k + 1, i + 1), at(k + 1, i)], inward3(right, mx, my), lining);
        }
        // The invert under the carriageway and its kerbs.
        let (l, r) = (lines[profile.len() - 1][i], lines[0][i]);
        let (l1, r1) = (lines[profile.len() - 1][i + 1], lines[0][i + 1]);
        g.part(Concrete).quad_facing([Vec3::new(l.x, 0.01, l.y), Vec3::new(l1.x, 0.01, l1.y), Vec3::new(r1.x, 0.01, r1.y), Vec3::new(r.x, 0.01, r.y)], Vec3::Y, rib);
    }
    // Lamp lines high on each wall, a hand's breadth proud of the lining.
    for side in [-1.0f32, 1.0] {
        let x = side * (hw + TUNNEL_WALL - 0.06);
        let line = offset_path(walk, x);
        for i in 0..line.len() - 1 {
            let d = (walk[i + 1] - walk[i]).normalize_or_zero();
            let into = Vec3::new(d.y * side, 0.0, -d.x * side);
            let (a, b) = (line[i], line[i + 1]);
            let (y0, y1) = (BORE_SPRING - 0.45, BORE_SPRING - 0.2);
            g.part(Glow).quad_facing([Vec3::new(a.x, y0, a.y), Vec3::new(b.x, y0, b.y), Vec3::new(b.x, y1, b.y), Vec3::new(a.x, y1, a.y)], into, lamp);
        }
    }
    // Lining bands: a ring of quads standing a little proud of the lining.
    let len = path_length(walk);
    let mut s = BORE_RIB_EVERY * 0.5;
    while s < len - 1.0 {
        let (p0, d0) = point_along(walk, (s - 0.35).max(0.0));
        let (p1, d1) = point_along(walk, (s + 0.35).min(len));
        let (r0, r1) = (Vec2::new(-d0.y, d0.x), Vec2::new(-d1.y, d1.x));
        let ring = |p: Vec2, right: Vec2, k: usize| {
            let (x, y) = profile[k];
            let inset = bore_inward(x, y) * 0.14;
            let q = p + right * (x + inset.x);
            Vec3::new(q.x, y + inset.y, q.y)
        };
        for k in 0..profile.len() - 1 {
            let (mx, my) = ((profile[k].0 + profile[k + 1].0) * 0.5, (profile[k].1 + profile[k + 1].1) * 0.5);
            g.part(Concrete).quad_facing([ring(p0, r0, k), ring(p1, r1, k), ring(p1, r1, k + 1), ring(p0, r0, k + 1)], inward3(r0, mx, my), rib);
        }
        s += BORE_RIB_EVERY;
    }
}

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

/// A node whose roads all run underground, drawn with the tunnels. A node
/// with no live roads at all is not one (it is drawn with nothing).
fn tunnel_node(roads: &RoadNetwork, n: NodeId) -> bool {
    let mut live = roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive).peekable();
    live.peek().is_some() && live.all(|&s| roads.segment(s).tunnel)
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
    // A path or a track ends where it crosses a street: it is drawn up to
    // the street's pavements (`footpath`, `railway`), not across its lanes.
    if kind != RoadKind::Street && roads.street_degree(node) > 0 {
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

/// Metres of taper per metre of width gained or lost, and its bounds.
const TAPER_PER_M: f32 = 9.0;
const TAPER_MIN: f32 = 12.0;
const TAPER_MAX: f32 = 45.0;

/// At a bend node `n` (two streets, `alive`) where the carriageway changes
/// width: the run of the narrower street, its path from `n` (as long as
/// the taper), and the half widths at `n` (the wider street's) and at the
/// taper's end. None where both are as wide, where the narrower is a town
/// street, or where either is a ring, tunnel, bridge or viaduct (decks and
/// bores have widths of their own).
fn lane_taper_at(roads: &RoadNetwork, runs: &[Run], n: NodeId, alive: &[SegmentId]) -> Option<(usize, Vec<Vec2>, f32, f32)> {
    let (sa, sb) = (roads.segment(alive[0]), roads.segment(alive[1]));
    if [sa, sb].iter().any(|s| s.ring || s.tunnel || s.bridge || s.viaduct) {
        return None;
    }
    let (wide, narrow) = if sa.half_width() >= sb.half_width() { (sa, alive[1]) } else { (sb, alive[0]) };
    let (hw0, hw1) = (wide.half_width(), roads.segment(narrow).half_width());
    // A town street's pavements and kerbs would cover the taper (they
    // step at the joint as before): only a highway or ramp, which has none.
    if hw0 - hw1 < 0.5 || !roads.segment(narrow).is_motorway() {
        return None;
    }
    let s = roads.segment(narrow);
    let far = if s.a == n { s.b } else { s.a };
    // The narrower street's run, from `n` outwards.
    let (ri, path) = runs.iter().enumerate().find_map(|(ri, r)| {
        let k = r.nodes.len();
        if k >= 2 && r.nodes[0] == n && r.nodes[1] == far {
            Some((ri, r.points.clone()))
        } else if k >= 2 && r.nodes[k - 1] == n && r.nodes[k - 2] == far {
            Some((ri, r.points.iter().rev().copied().collect::<Vec<_>>()))
        } else {
            None
        }
    })?;
    let len = ((hw0 - hw1) * TAPER_PER_M).clamp(TAPER_MIN, TAPER_MAX).min(path_length(&path) * 0.8);
    if len < 2.0 {
        return None;
    }
    // The path up to the taper's end.
    let mut out = vec![path[0]];
    let mut acc = 0.0;
    for w in path.windows(2) {
        let d = w[0].distance(w[1]);
        if acc + d >= len {
            out.push(w[0].lerp(w[1], (len - acc) / d.max(1e-6)));
            break;
        }
        acc += d;
        out.push(w[1]);
    }
    Some((ri, out, hw0, hw1))
}

/// A carriageway along `path` whose half width eases from `hw0` at its
/// start to `hw1` at its end, with its edge lines: the taper where a road
/// gains or drops lanes, drawn just over the narrower road's asphalt; with
/// `barrier`, a highway's low concrete barrier along both edges.
fn lane_taper(g: &mut Geometry, path: &[Vec2], hw0: f32, hw1: f32, asphalt: [f32; 4], edge: [f32; 4], barrier: Option<[f32; 4]>) {
    let total = path_length(path);
    if total < 0.5 {
        return;
    }
    // A vertex every couple of metres, so a bend keeps its shape.
    let steps = ((total / 2.0).ceil() as usize).clamp(2, 40);
    let samples: Vec<(Vec2, Vec2, f32)> = (0..=steps)
        .map(|k| {
            let t = k as f32 / steps as f32;
            let (p, d) = point_along(path, total * t);
            // Smoothstep: the edge leaves the wide road's edge and meets
            // the narrow one's tangentially.
            let e = t * t * (3.0 - 2.0 * t);
            (p, Vec2::new(-d.y, d.x), hw0 + (hw1 - hw0) * e)
        })
        .collect();
    for w in samples.windows(2) {
        let ((p0, n0, h0), (p1, n1, h1)) = (w[0], w[1]);
        g.part(Asphalt).ground_polygon(&[p0 - n0 * h0, p1 - n1 * h1, p1 + n1 * h1, p0 + n0 * h0], ASPHALT_Y + 0.008, asphalt);
    }
    for side in [-1.0f32, 1.0] {
        let line: Vec<Vec2> = samples.iter().map(|&(p, n, h)| p + n * side * (h - SHOULDER * 0.5)).collect();
        g.part(Paint).ground_strip(&line, 0.18, MARK_Y + 0.003, edge);
        // A highway's or ramp's barrier follows the taper's edge.
        if let Some(c) = barrier {
            let rail: Vec<Vec2> = samples.iter().map(|&(p, n, h)| p + n * side * (h + 0.2)).collect();
            g.part(Concrete).path_box(&rail, 0.0, 0.4, 0.0, 0.8, c);
        }
    }
}

/// A polyline whose points lie on a circle, with points added along the
/// circle between them (about every 3 m): its legs are chords otherwise.
fn arc_points(points: &[Vec2], centre: Vec2, radius: f32) -> Vec<Vec2> {
    let mut out = Vec::with_capacity(points.len() * 4);
    for (i, w) in points.windows(2).enumerate() {
        let (a0, a1) = ((w[0] - centre).to_angle(), (w[1] - centre).to_angle());
        let mut da = a1 - a0;
        while da > std::f32::consts::PI {
            da -= std::f32::consts::TAU;
        }
        while da < -std::f32::consts::PI {
            da += std::f32::consts::TAU;
        }
        let n = ((da.abs() * radius / 3.0).ceil() as usize).max(1);
        if i == 0 {
            out.push(w[0]);
        }
        for k in 1..n {
            out.push(centre + Vec2::from_angle(a0 + da * k as f32 / n as f32) * radius);
        }
        out.push(w[1]);
    }
    out
}

/// [sweep-visual] Distance from a polyline's start (`path`, from a
/// junction outward) at or past `from` where every point at the lateral
/// `offsets` (left of travel positive, as `offset_path`) lies clear of each
/// of `others` (a polyline and its half width): where a sharp-angled arm
/// stops overlapping its neighbours, followed round its bends past its
/// first piece. Searched in half-metre steps over at most `limit`; `from`
/// when nothing clears by then.
fn clear_along(path: &[Vec2], from: f32, limit: f32, offsets: &[f32], others: &[(Vec<Vec2>, f32)]) -> f32 {
    if others.is_empty() || path.len() < 2 {
        return from;
    }
    let near = |c: Vec2, q: &[Vec2]| q.windows(2).map(|w| nd_sim::geom::project_on_segment(c, w[0], w[1]).1.distance(c)).fold(f32::MAX, f32::min);
    let mut s = from;
    while s <= limit {
        let (p, d) = point_along(path, s);
        let n = Vec2::new(-d.y, d.x);
        if offsets.iter().all(|o| others.iter().all(|(q, hw)| near(p + n * *o, q) >= *hw + 0.05)) {
            return s;
        }
        s += 0.5;
    }
    from
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

/// The lines of a ramp's join with a highway carriageway at a node, as
/// straight lines from the node: the carriageway's edge line on the ramp's
/// side and the ramp's edge lines and outer asphalt edge.
struct GoreLines {
    at: Vec2,
    dh: Vec2,
    dr: Vec2,
    /// Unit normals of each way toward the other.
    nh: Vec2,
    nr: Vec2,
    /// The carriageway's edge line, the ramp's edge lines and its asphalt
    /// half width.
    hh: f32,
    hr: f32,
    hw_r: f32,
    /// Along the ramp: where its inner edge line has left the carriageway's
    /// (the gore's point), and where its asphalt is clear of the
    /// carriageway's (its own strip begins).
    parted: f32,
    clear: f32,
    /// [interchange] Length of the ramp's first piece (its straight lead).
    len: f32,
}

impl GoreLines {
    fn eh(&self, s: f32) -> Vec2 {
        self.at + self.dh * s + self.nh * self.hh
    }
    /// The ramp's inner edge line.
    fn er(&self, s: f32) -> Vec2 {
        self.at + self.dr * s + self.nr * self.hr
    }
    /// The ramp's outer asphalt edge, and its outer edge line.
    fn eo(&self, s: f32) -> Vec2 {
        self.at + self.dr * s - self.nr * self.hw_r
    }
    fn eo_line(&self, s: f32) -> Vec2 {
        self.at + self.dr * s - self.nr * self.hr
    }
    /// How far a point lies out from the carriageway's edge line.
    fn out(&self, s: f32, p: Vec2) -> f32 {
        (p - self.eh(s)).dot(self.nh)
    }
}

fn gore_lines(roads: &RoadNetwork, node: NodeId, ramp: SegmentId) -> Option<GoreLines> {
    let at = roads.node(node).pos;
    let away = |s: SegmentId| {
        let seg = roads.segment(s);
        let far = if seg.a == node { seg.b } else { seg.a };
        (roads.node(far).pos - at).normalize_or_zero()
    };
    let dr = away(ramp);
    let alive: Vec<SegmentId> = roads.node(node).segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
    // The carriageway on the ramp's side of the node: the one it runs along.
    let hw_seg = alive.iter().copied().filter(|&s| s != ramp).max_by(|&x, &y| away(x).dot(dr).total_cmp(&away(y).dot(dr)))?;
    let dh = away(hw_seg);
    let hw_r = roads.segment(ramp).half_width();
    let (hh, hr) = (roads.segment(hw_seg).half_width() - SHOULDER * 0.5, hw_r - SHOULDER * 0.5);
    let nh = { let n = Vec2::new(-dh.y, dh.x); if n.dot(dr) >= 0.0 { n } else { -n } };
    let nr = { let n = Vec2::new(-dr.y, dr.x); if n.dot(dh) >= 0.0 { n } else { -n } };
    let len = { let (a, b) = roads.segment_points(ramp); a.distance(b) };
    let mut g = GoreLines { at, dh, dr, nh, nr, hh, hr, hw_r, parted: f32::INFINITY, clear: f32::INFINITY, len };
    let mut s = 0.0f32;
    while s <= len.min(150.0) {
        let gap = g.out(s, g.er(s));
        if gap > 0.15 && g.parted.is_infinite() {
            g.parted = s;
        }
        // Clear when the two asphalt edges have parted (each 0.75 m out
        // past its edge line).
        if gap > SHOULDER + 0.05 {
            g.clear = s;
            break;
        }
        s += 0.5;
    }
    g.parted.is_finite().then_some(g)
}

/// Where a ramp joins a highway (or leaves it): the ramp's lane widening
/// out from the carriageway's edge (asphalt from the carriageway's edge line
/// to the ramp's outer edge, with the ramp's outer edge line) until the two
/// are clear of each other, and chevron hatching in the gore beyond its
/// point. The ramp's own strip and markings begin where it is clear
/// (`GoreLines::clear`): drawn from the node on the carriageway's centre
/// line, its edge lines used to cross the carriageway's lanes in an X.
fn ramp_gore(g: &mut Geometry, roads: &RoadNetwork, node: NodeId, ramp: SegmentId, asphalt: [f32; 4], paint: [f32; 4]) {
    let Some(gl) = gore_lines(roads, node, ramp) else { return };
    // The widening: from where the ramp's outer edge leaves the carriageway
    // to where the ramp is clear.
    let mut taper_h: Vec<Vec2> = Vec::new();
    let mut taper_o: Vec<Vec2> = Vec::new();
    let mut outline: Vec<Vec2> = Vec::new();
    let end = if gl.clear.is_finite() { gl.clear } else { gl.parted + 40.0 };
    let mut s = 0.0f32;
    while s <= end + 1e-3 {
        if gl.out(s, gl.eo(s)) > 0.0 {
            taper_h.push(gl.eh(s));
            taper_o.push(gl.eo(s));
        }
        if gl.out(s, gl.eo_line(s)) > 0.0 {
            outline.push(gl.eo_line(s));
        }
        s = if s < end && s + 0.5 > end { end } else { s + 0.5 };
    }
    if taper_h.len() >= 2 {
        // Strips of quads: the outline bends where the ramp leaves the
        // carriageway, and one polygon of it is not convex.
        for k in 0..taper_h.len() - 1 {
            g.part(Asphalt).ground_polygon(&[taper_h[k], taper_h[k + 1], taper_o[k + 1], taper_o[k]], ASPHALT_Y + 0.004, asphalt);
        }
    }
    if outline.len() >= 2 {
        g.part(Paint).ground_strip(&outline, 0.18, MARK_Y, paint);
    }
    // The gore beyond its point: paved between the two edge lines, outlined
    // in solid wide lines and hatched with chevrons.
    let (mut side_h, mut side_r): (Vec<Vec2>, Vec<Vec2>) = (Vec::new(), Vec::new());
    let mut s = gl.parted;
    // [interchange] No further than the ramp's first piece runs straight:
    // past its bend the gore's lines ran on over the grass (a short ramp's
    // lead piece is 20 m, the hatching 40 m), a big chevron triangle beside
    // a ramp already turning away.
    let gore_end = (gl.parted + 40.0).min(gl.len);
    while s <= gore_end {
        let (eh, er) = (gl.eh(s), gl.er(s));
        let gap = gl.out(s, er);
        side_h.push(eh);
        side_r.push(er);
        // Chevrons across the gore every 4 m, at road-marking scale:
        // 0.3 m strokes every 3 m read as a grey smear from the top-down
        // camera. Each stroke is 0.6 m wide, and the V's point is drawn 2 m
        // along so the arms sweep at a clear angle.
        let k = s - gl.parted;
        if k >= 1.0 && (k as i32) % 4 == 1 && (k - k.floor()) < 0.25 && gap > 1.2 {
            let mid = (eh + er) * 0.5 + (gl.dh + gl.dr).normalize_or_zero() * 2.0;
            g.part(Paint).ground_strip(&[eh + (er - eh) * 0.1, mid], 0.6, MARK_Y + 0.004, paint);
            g.part(Paint).ground_strip(&[mid, er - (er - eh) * 0.1], 0.6, MARK_Y + 0.004, paint);
        }
        s += 0.5;
    }
    if side_h.len() >= 2 {
        // Above the widening's asphalt, which it overlaps up to `clear`.
        for k in 0..side_h.len() - 1 {
            g.part(Asphalt).ground_polygon(&[side_h[k], side_h[k + 1], side_r[k + 1], side_r[k]], ASPHALT_Y + 0.007, asphalt);
        }
        g.part(Paint).ground_strip(&side_h, 0.3, MARK_Y + 0.002, paint);
        g.part(Paint).ground_strip(&side_r, 0.3, MARK_Y + 0.002, paint);
    }
}

/// A run's height profile: its points with the node heights, and between
/// them the inner samples of any long street (`World::road_inner`) as
/// extra points, so the run is lifted onto the way the cars drive on.
fn run_profile(roads: &RoadNetwork, run: &Run, node_z: &impl Fn(NodeId) -> f32, inner: &[Vec<(f32, f32)>]) -> (Vec<Vec2>, Vec<f32>) {
    let mut pts = vec![run.points[0]];
    let mut z = vec![node_z(run.nodes[0])];
    for k in 0..run.points.len() - 1 {
        let (na, nb) = (run.nodes[k], run.nodes[k + 1]);
        let seg = roads.node(na).segments.iter().copied().find(|&s| {
            let sg = roads.segment(s);
            sg.alive && ((sg.a == na && sg.b == nb) || (sg.a == nb && sg.b == na))
        });
        if let Some(samples) = seg.and_then(|s| inner.get(s.0 as usize).filter(|v| !v.is_empty()).map(|v| (roads.segment(s).a == na, v))) {
            let (forward, v) = samples;
            let (a, b) = (run.points[k], run.points[k + 1]);
            let mut add = |t: f32, h: f32| {
                pts.push(a.lerp(b, t));
                z.push(h);
            };
            if forward {
                for &(t, h) in v.iter() {
                    add(t, h);
                }
            } else {
                for &(t, h) in v.iter().rev() {
                    add(1.0 - t, h);
                }
            }
        }
        pts.push(run.points[k + 1]);
        z.push(node_z(nb));
    }
    (pts, z)
}

/// Does `p` stand on a street's lanes at about height `z`? For the walk
/// view's clutter (`walk.rs`), as the street's own furniture.
pub(crate) fn on_carriageway(roads: &RoadNetwork, heights: &[f32], inner: &[Vec<(f32, f32)>], p: Vec2, z: f32) -> bool {
    junction::on_carriageway(roads, heights, inner, p, z, 0.05)
}

/// Vertex counts of every part: where the next piece of drawing starts.
pub(crate) fn marks(g: &Geometry) -> Vec<usize> {
    g.parts.iter().map(|m| m.positions.len()).collect()
}

/// Raise every vertex drawn since `from` by `h` of its (x, z).
pub(crate) fn lift_since(g: &mut Geometry, from: &[usize], h: impl Fn(Vec2) -> f32) {
    for (k, m) in g.parts.iter_mut().enumerate() {
        let start = from.get(k).copied().unwrap_or(0);
        for p in m.positions.iter_mut().skip(start) {
            p[1] += h(Vec2::new(p[0], p[2]));
        }
    }
}

/// Height along a run at the point of it nearest `p`, from the heights of
/// its nodes. This scans every leg of the run, which is fine for one point
/// but not for every vertex of a long run: the street generator uses
/// [`RunProfile`], which gives the same answer from a grid of legs.
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

/// A run's height profile with its legs binned on a grid, so lifting every
/// vertex drawn for the run costs a few leg tests each instead of one per
/// leg of the run. Setting runs on their profile used to scan the whole run
/// per vertex: the 10 km highway has ~90 legs and tens of thousands of
/// vertices (barrier pieces every 2 m), and with relief on every road edit
/// stalled the frame for about a second.
///
/// Each leg is listed in every cell within `reach` of it, so for a vertex
/// within `reach` of the run its nearest leg is among its cell's candidates
/// and the answer is exactly [`profile_height`]'s (candidates are kept in
/// leg order and ties go to the earlier leg, as the scan does). Anything
/// farther out (nothing a street draws) falls back to the full scan.
pub struct RunProfile<'a> {
    points: &'a [Vec2],
    z: &'a [f32],
    cell: f32,
    reach: f32,
    cells: std::collections::HashMap<(i32, i32), Vec<u32>>,
}

impl<'a> RunProfile<'a> {
    /// Below this many legs the plain scan is as quick as the grid.
    const SMALL: usize = 6;

    pub fn new(points: &'a [Vec2], z: &'a [f32], reach: f32) -> Self {
        let cell = 24.0f32;
        let mut cells: std::collections::HashMap<(i32, i32), Vec<u32>> = Default::default();
        if points.len() > Self::SMALL + 1 && z.len() == points.len() {
            // The farthest a cell's corner lies from its centre: a cell is
            // listed for a leg if any point of it may be within `reach`.
            let slack = reach + cell * std::f32::consts::FRAC_1_SQRT_2;
            for (i, w) in points.windows(2).enumerate() {
                let lo = w[0].min(w[1]) - Vec2::splat(reach);
                let hi = w[0].max(w[1]) + Vec2::splat(reach);
                let (x0, y0) = ((lo.x / cell).floor() as i32, (lo.y / cell).floor() as i32);
                let (x1, y1) = ((hi.x / cell).floor() as i32, (hi.y / cell).floor() as i32);
                for cx in x0..=x1 {
                    for cy in y0..=y1 {
                        let c = Vec2::new((cx as f32 + 0.5) * cell, (cy as f32 + 0.5) * cell);
                        if nd_sim::roads::project_on_segment(c, w[0], w[1]).1.distance(c) <= slack {
                            cells.entry((cx, cy)).or_default().push(i as u32);
                        }
                    }
                }
            }
        }
        Self { points, z, cell, reach, cells }
    }

    /// The height at the point of the run nearest `p`.
    pub fn height(&self, p: Vec2) -> f32 {
        if self.cells.is_empty() {
            return profile_height(self.points, self.z, p);
        }
        let key = ((p.x / self.cell).floor() as i32, (p.y / self.cell).floor() as i32);
        let mut best = (f32::INFINITY, 0.0f32);
        for &i in self.cells.get(&key).map(|v| v.as_slice()).unwrap_or(&[]) {
            let i = i as usize;
            let (t, q) = nd_sim::roads::project_on_segment(p, self.points[i], self.points[i + 1]);
            let d = q.distance_squared(p);
            if d < best.0 {
                best = (d, self.z[i] + (self.z[i + 1] - self.z[i]) * t);
            }
        }
        if best.0 <= self.reach * self.reach {
            best.1
        } else {
            profile_height(self.points, self.z, p)
        }
    }
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
    let runs = build_runs(roads);
    // Each run's height profile (none on a flat run), kept for the junction
    // corners, which are set on the profiles of the two runs they join.
    let profiles: Vec<(Vec<Vec2>, Vec<f32>)> = if heights.is_empty() { Vec::new() } else { runs.iter().map(|r| run_profile(roads, r, &node_z, inp.inner)).collect() };
    // Everything a street draws stands within its pavements and wires (or a
    // ramp's gore): well inside this reach.
    let lifts: Vec<Option<RunProfile>> = profiles.iter().map(|(p, z)| z.iter().any(|h| *h != 0.0).then(|| RunProfile::new(p, z, 40.0))).collect();
    let run_z = |ri: usize, p: Vec2| lifts.get(ri).and_then(|l| l.as_ref()).map_or(0.0, |l| l.height(p));
    let corners = junction_corners(roads, net, &runs);
    // Whether each run is drawn worn (as the run loop decides it).
    let run_worn: Vec<bool> = runs.iter().map(|r| inp.land_value.at(point_along(&r.points, path_length(&r.points) * 0.5).0) < 0.35).collect();
    // The run arriving at each node on each segment.
    let mut arm_run: HashMap<(u32, u32), usize> = HashMap::new();
    for (ri, r) in runs.iter().enumerate() {
        let k = r.nodes.len();
        if let Some(sg) = segment_between(roads, r.nodes[0], r.nodes[1]) {
            arm_run.insert((r.start_node.0, sg.0), ri);
        }
        if let Some(sg) = segment_between(roads, r.nodes[k - 1], r.nodes[k - 2]) {
            arm_run.insert((r.end_node.0, sg.0), ri);
        }
    }
    // Where a road carries on with fewer or more lanes: (node, narrower
    // run, taper path from the node, half widths at its two ends).
    let tapers: Vec<(NodeId, usize, Vec<Vec2>, f32, f32)> = (0..roads.nodes.len())
        .filter_map(|i| {
            let n = NodeId(i as u32);
            let alive: Vec<SegmentId> = roads.node(n).segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
            if alive.len() != 2 || roads.node(n).segments.iter().any(|&s| roads.segment(s).alive && !roads.segment(s).is_street()) {
                return None;
            }
            lane_taper_at(roads, &runs, n, &alive).map(|(ri, path, h0, h1)| (n, ri, path, h0, h1))
        })
        .collect();
    let cuts = crossing_cuts(roads);
    // Each junction's arms with their mouths (`junction::mouths`), and each
    // ramp's join with a highway: where the arms' own strips and markings
    // begin.
    let pad_mouths: HashMap<u32, Vec<Mouth>> = (0..roads.nodes.len() as u32).filter(|&i| pad_node(roads, NodeId(i))).map(|i| (i, mouths(roads, net, NodeId(i)))).collect();
    let gores: HashMap<u32, (SegmentId, f32)> = (0..roads.nodes.len() as u32)
        .filter_map(|i| {
            let ramp = gore_ramp(roads, NodeId(i))?;
            let gl = gore_lines(roads, NodeId(i), ramp)?;
            Some((i, (ramp, if gl.clear.is_finite() { gl.clear } else { gl.parted })))
        })
        .collect();
    // (carriageway, markings) set-back of a run's end at `node` on `seg`.
    let end_trim = |node: NodeId, seg: Option<SegmentId>| -> (f32, f32) {
        let Some(seg) = seg else { return (0.0, 0.0) };
        if let Some(m) = pad_mouths.get(&node.0).and_then(|v| v.iter().find(|m| m.seg == seg)) {
            return (m.at, m.marks);
        }
        match gores.get(&node.0) {
            Some(&(ramp, d)) if ramp == seg => (d, d),
            _ => (0.0, 0.0),
        }
    };
    // [sweep-visual] The other arms of a pad junction, as polylines with
    // their half widths (only those that front nothing, a ramp or a
    // highway, with `ramps_only`); and whether an arm's overlap with them
    // runs on past its first piece.
    let others_at = |node: NodeId, seg: Option<SegmentId>, ramps_only: bool| -> Vec<(Vec<Vec2>, f32)> {
        let Some(list) = pad_mouths.get(&node.0) else { return Vec::new() };
        list.iter()
            .filter(|m| Some(m.seg) != seg && (!ramps_only || roads.segment(m.seg).fronts_nothing()))
            .filter_map(|m| arm_run.get(&(node.0, m.seg.0)).map(|&ri| (runs[ri].points.clone(), roads.segment(m.seg).half_width())))
            .collect()
    };
    let overlaps_on = |node: NodeId, seg: Option<SegmentId>| pad_mouths.get(&node.0).and_then(|v| seg.and_then(|s| v.iter().find(|m| m.seg == s))).is_some_and(|m| m.want > m.at + 0.5);
    let kerb_cols = KerbColours { kerb: kerb_c, paving: paving_c, tactile: rgba(pal.tactile) };
    // A street that just ends (no other street, path or track at its end):
    // kerbed round its end, no pad.
    let dead_end = |n: NodeId| roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive).count() == 1 && roads.street_degree(n) == 1;
    // The run drawn last and where its drawing began, to set it on its
    // profile once it is complete (runs end at several `continue`s).
    let mut drawn: Option<(Vec<usize>, usize)> = None;
    let finish_run = |g: &mut Geometry, drawn: &mut Option<(Vec<usize>, usize)>| {
        if let Some((from, ri)) = drawn.take() {
            if let Some(Some(profile)) = lifts.get(ri) {
                lift_since(g, &from, |p| profile.height(p));
            }
        }
    };
    for (ri, run) in runs.iter().enumerate() {
        finish_run(&mut g, &mut drawn);
        if let Some(marks) = worn_from.take() {
            weather_tail(&mut g, marks);
        }
        // The run is drawn through its profile's points: a long street's
        // inner height samples become vertices of its carriageway, kerbs
        // and pavements. Drawn through its nodes alone, a street over a
        // rise ran straight from end to end under the profile the ground
        // and the cars follow, and the built ground covered its asphalt.
        let dense = if heights.is_empty() {
            run.points.clone()
        } else {
            drawn = Some((marks(&g), ri));
            profiles[ri].0.clone()
        };
        let seg = roads.segment(run.first);
        if !inp.layer.keeps(seg.tunnel) {
            continue;
        }
        let hw = seg.half_width();
        let total = run.lanes_fwd as f32 + run.lanes_bwd as f32;
        let mut rng = rng_for(inp.seed ^ (run.first.0 as u64 * 7919), 3);
        // A roundabout's ring follows its circle, a vertex every 3 m or so:
        // drawn through its nodes alone it was a polygon, its outer
        // pavement a run of straight facets and its inner edge cutting into
        // the island.
        let on_circle = seg.ring.then(|| roads.ring_of_segment(run.first).and_then(|k| roads.rings.get(k))).flatten().map(|ring| arc_points(&dense, ring.center, ring.radius));
        let pts = on_circle.as_ref().unwrap_or(&dense);
        if path_length(pts) < 0.5 {
            continue;
        }
        if run.kind == RoadKind::Path {
            footpath(&mut g, pal, roads, run, pts, &mut rng);
            continue;
        }
        if run.kind == RoadKind::Rail {
            railway(&mut g, pal, roads, run, pts);
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
        // A roundabout's ring has a kerb and pavement round its outside only,
        // which the arms' corners run into; its inner edge is the mountable
        // apron. The side of the run that faces out:
        let ring_out = seg.ring.then(|| roads.ring_of_segment(run.first).and_then(|k| roads.rings.get(k))).flatten().map(|ring| {
            let (mid, d) = point_along(pts, path_length(pts) * 0.5);
            if (mid + Vec2::new(-d.y, d.x) * hw).distance(ring.center) > ring.radius { 1.0f32 } else { -1.0 }
        });
        // The carriageway stops at each junction's mouth, where the pad takes
        // over, and the markings at the stop line (`junction::mouths`); a
        // ramp's at the point it is clear of the highway it joins.
        let nn = run.nodes.len();
        let (c0, m0) = end_trim(run.start_node, segment_between(roads, run.nodes[0], run.nodes[1]));
        let (c1, m1) = end_trim(run.end_node, segment_between(roads, run.nodes[nn - 1], run.nodes[nn - 2]));
        let cut = |a: f32, b: f32| if a == 0.0 && b == 0.0 { pts.clone() } else { trim(pts, a, b) };
        // [sweep-visual] An arm meeting a neighbour at so sharp an angle
        // that they overlap past its first piece (a ramp drawn into a
        // street): its own strip starts where it is clear of them, its
        // stretch over them drawn a little under their asphalt (no
        // coplanar flicker; it shows only where it runs out beside them)
        // and unmarked.
        let run_len = path_length(pts);
        let rev: Vec<Vec2> = pts.iter().rev().copied().collect();
        let (seg0, seg1) = (segment_between(roads, run.nodes[0], run.nodes[1]), segment_between(roads, run.nodes[nn - 1], run.nodes[nn - 2]));
        let s0 = if overlaps_on(run.start_node, seg0) { clear_along(pts, c0, run_len * 0.45, &[-hw, 0.0, hw], &others_at(run.start_node, seg0, false)) } else { c0 };
        let s1 = if overlaps_on(run.end_node, seg1) { clear_along(&rev, c1, run_len * 0.45, &[-hw, 0.0, hw], &others_at(run.end_node, seg1, false)) } else { c1 };
        let carriage = cut(s0, s1);
        let marked = cut(m0.max(s0), m1.max(s1));
        if carriage.len() >= 2 {
            g.part(Asphalt).path_strip(&carriage, 0.0, hw * 2.0, ASPHALT_Y, asphalt_c);
        }
        for under in [(s0 > c0 + 0.1).then(|| trim(pts, c0, (run_len - s0).max(0.0))), (s1 > c1 + 0.1).then(|| trim(pts, (run_len - s1).max(0.0), c1))].into_iter().flatten() {
            if under.len() >= 2 {
                g.part(Asphalt).path_strip(&under, 0.0, hw * 2.0, ASPHALT_Y - 0.03, asphalt_c);
            }
        }
        // Round a ring only the outer edge line is broken at the arms.
        let ring_lines = if ring_out.is_some() { pts.clone() } else { marked.clone() };
        for side in [-1.0f32, 1.0] {
            let line = if ring_out == Some(side) { &marked } else { &ring_lines };
            if line.len() >= 2 {
                g.part(Paint).path_strip(line, side * (hw - SHOULDER * 0.5), 0.18, MARK_Y, fade(edge_c));
            }
        }
        if ring_lines.len() >= 2 {
            for k in 1..(run.lanes_fwd + run.lanes_bwd) as u32 {
                let off = (k as f32 - total * 0.5) * LANE_WIDTH;
                let is_centre = run.lanes_fwd > 0 && run.lanes_bwd > 0 && k == run.lanes_bwd as u32;
                if is_centre {
                    g.part(Paint).path_strip(&ring_lines, off + 0.18, 0.14, MARK_Y, fade(centre_c));
                    g.part(Paint).path_strip(&ring_lines, off - 0.18, 0.14, MARK_Y, fade(centre_c));
                } else {
                    let path = offset_path(&ring_lines, off);
                    dashes_along(g.part(Paint), &path, 0.16, MARK_Y, 3.0, 4.0, fade(divider_c));
                }
            }
            // One-way roads (ramps, highways, one-way streets) show which way
            // they run: an arrow in every lane every 45 m.
            if (run.lanes_fwd == 0) != (run.lanes_bwd == 0) {
                let forward = run.lanes_fwd > 0;
                for k in 0..(run.lanes_fwd + run.lanes_bwd) {
                    let path = offset_path(&ring_lines, (k as f32 + 0.5 - total * 0.5) * LANE_WIDTH);
                    travel_arrows(g.part(Paint), &path, forward, fade(edge_c));
                }
            }
        }

        // Kerbs and pavements stop short of the junction pads, each side as
        // far back as the rounded corner it runs into needs
        // (`junction_corners`); street furniture keeps to this stretch.
        let t0 = node_r(run.start_node) + 0.6;
        let t1 = node_r(run.end_node) + 0.6;
        let walk = trim(pts, t0, t1);
        if walk.len() < 2 {
            continue;
        }
        // Highways and ramps have hard shoulders, not pavements: a low
        // barrier at the edge and lamps, nothing else beside them.
        let motorway = seg.is_motorway() || (seg.ring && ring_out.is_none());
        if motorway {
            let barrier = rgba(mul(pal.concrete(0), 0.8));
            // Where a ramp runs beside the carriageway to merge or diverge,
            // neither barrier stands between them: a barrier stops where it
            // would be on (or right beside) another motorway's lanes.
            let mine = |sid: SegmentId| {
                let s = roads.segment(sid);
                run.nodes.contains(&s.a) && run.nodes.contains(&s.b)
            };
            // Only a way at this one's height counts: a tunnel (or any way
            // passing well under or over) is not beside it, and the
            // barrier used to stop exactly where a tunnel ramp ran beneath.
            // The run's own height comes from its binned profile (`lifts`),
            // inner samples and all: a list of node heights against the
            // run's points read the first node's height everywhere.
            // Along a taper at either end the barrier follows its edge
            // (`lane_taper`), not the narrower road's.
            let in_taper = |p: Vec2| {
                tapers.iter().filter(|t| t.1 == ri).any(|(_, _, path, h0, _)| {
                    let total = path_length(path);
                    let mut acc = 0.0;
                    path.windows(2).any(|w| {
                        let (t, q) = nd_sim::roads::project_on_segment(p, w[0], w[1]);
                        let along = acc + w[0].distance(w[1]) * t;
                        acc += w[0].distance(w[1]);
                        along < total - 0.3 && q.distance(p) < h0 + 1.0
                    })
                })
            };
            let blocked = |p: Vec2| {
                in_taper(p) || roads.segments_in_box(p - Vec2::splat(30.0), p + Vec2::splat(30.0)).any(|o| {
                    let os = roads.segment(o);
                    if mine(o) || !(os.is_motorway() || os.ring) || os.tunnel != seg.tunnel {
                        return false;
                    }
                    let (a, b) = roads.segment_points(o);
                    let (t, q) = nd_sim::roads::project_on_segment(p, a, b);
                    let oz = nd_sim::landscape::way_height(roads, heights, inp.inner, o, t);
                    q.distance(p) < os.half_width() + 1.2 && (oz - run_z(ri, p)).abs() < 2.5
                })
            };
            for side in [-1.0f32, 1.0] {
                let line = offset_path(&walk, side * (hw + 0.2));
                let len = path_length(&line);
                let steps = (len / 2.0).ceil().max(1.0) as usize;
                let mut piece: Vec<Vec2> = Vec::new();
                for k in 0..=steps {
                    let q = point_along(&line, len * k as f32 / steps as f32).0;
                    if blocked(q) {
                        if piece.len() >= 2 {
                            g.part(Concrete).path_box(&offset_path(&piece, 0.0), 0.0, 0.4, 0.0, 0.8, barrier);
                        }
                        piece.clear();
                    } else {
                        piece.push(q);
                    }
                }
                if piece.len() >= 2 {
                    g.part(Concrete).path_box(&piece, 0.0, 0.4, 0.0, 0.8, barrier);
                }
            }
            let len = path_length(&walk);
            let mut s = 10.0;
            let mut side = 1.0;
            while s < len - 4.0 {
                let (p, d) = point_along(&walk, s);
                let right = Vec2::new(-d.y, d.x);
                // Not on the lanes of a ramp running alongside to merge.
                let q = p + right * side * (hw + 0.5);
                if !junction::on_carriageway(roads, heights, inp.inner, q, run_z(ri, q), 0.05) {
                    lamp_post_world(&mut g, q, 9.0, pal.street_lamp);
                }
                s += 40.0;
                side = -side;
            }
            continue;
        }
        if seg.tunnel {
            // The bore: like a ramp, a way for traffic only (nobody walks
            // or parks in it), so no pavements: a low safety kerb at the
            // shoulder, then the tube (`bore_shell`) with its lamp lines and
            // no street furniture. `nd_procgen::terrain` sizes the portal
            // arch to the tube's walls (`TUNNEL_WALL`).
            for side in [-1.0f32, 1.0] {
                g.part(Concrete).path_box(&walk, side * (hw + 0.2), 0.4, 0.0, 0.35, kerb_c);
            }
            bore_shell(&mut g, &walk, hw, rgba(mul(pal.concrete(0), 0.75)), rgba(mul(pal.concrete(0), 0.5)), rgba(mul(pal.street_lamp, 0.9)));
            continue;
        }
        // Kerbs and pavements, each side to its own corners, dropped
        // wherever a crosswalk meets them.
        let run_cuts: Vec<Cut> = run.nodes.windows(2).filter_map(|w| segment_between(roads, w[0], w[1])).flat_map(|s| cuts.get(&s.0).into_iter().flatten().copied()).collect();
        // Where each side's kerb and pavement begin and end (along the run).
        let mut kerb_ends = [(t0, t1); 2];
        for side in [-1.0f32, 1.0] {
            if ring_out.is_some_and(|o| o != side) {
                continue;
            }
            // Without a corner of its own, a kerb stays back until it is
            // clear of the junction's other arms; at a dead end it runs to
            // the end, where `cul_de_sac` closes it.
            let end = |node: NodeId, start: bool, t: f32| {
                if dead_end(node) {
                    return (0.0, None);
                }
                // [sweep-visual] Past a ramp or highway joined at a sharp
                // angle (which has no pavement or corner of its own on this
                // side) the kerb and pavement wait until they are clear of
                // its carriageway, however far round its bends that is.
                let seg = if start { segment_between(roads, run.nodes[0], run.nodes[1]) } else { segment_between(roads, run.nodes[nn - 1], run.nodes[nn - 2]) };
                let ramps = others_at(node, seg, true);
                let clear = if ramps.is_empty() {
                    0.0
                } else {
                    let (path, sgn) = if start { (pts.clone(), side) } else { (pts.iter().rev().copied().collect::<Vec<_>>(), -side) };
                    let from = node_r(node);
                    let c = clear_along(&path, from, path_length(&path) * 0.45, &[sgn * hw, sgn * (hw + SIDEWALK)], &ramps);
                    if c > from { c } else { 0.0 }
                };
                let e = match corners.trims.get(&(ri, start, side > 0.0)) {
                    Some(&e) => e,
                    None => {
                        // The run's +1 side is counter-clockwise of the way
                        // out where it starts here, clockwise where it ends.
                        let ccw = (side > 0.0) == start;
                        t.max(seg.map_or(0.0, |s| junction::kerb_clearance(pad_mouths.get(&node.0).map(|v| v.as_slice()).unwrap_or(&[]), s, ccw)))
                    }
                };
                (e.max(clear), (clear > e).then_some(e))
            };
            let ((e0, held0), (e1, held1)) = (end(run.start_node, true, t0), end(run.end_node, false, t1));
            kerb_ends[(side > 0.0) as usize] = (e0, e1);
            // [sweep-visual] Where the kerb waits for a ramp to clear, the
            // stretch the pavement would have taken is surfaced like the
            // junction (under the ramp's own asphalt), not left bare.
            let total = path_length(pts);
            for (start, held, e) in [(true, held0, e0), (false, held1, e1)] {
                let Some(h) = held else { continue };
                let band = if start { trim(pts, h, (total - e).max(0.0)) } else { trim(pts, (total - e).max(0.0), h) };
                if band.len() >= 2 {
                    g.part(Asphalt).path_strip(&band, side * (hw + SIDEWALK * 0.5), SIDEWALK, ASPHALT_Y - 0.03, asphalt_c);
                }
            }
            let line = if (e0, e1) == (t0, t1) { walk.clone() } else { trim(pts, e0, e1) };
            if line.len() >= 2 {
                kerb_ribbon(&mut g, &offset_path(&line, side * hw), None, side, SIDEWALK, true, true, &run_cuts, &kerb_cols);
            }
        }
        if ring_out.is_none() {
            for (start, node) in [(true, run.start_node), (false, run.end_node)] {
                if dead_end(node) {
                    let (p, d) = if start { let (p, d) = point_along(pts, 0.0); (p, -d) } else { point_along(pts, path_length(pts)) };
                    cul_de_sac(&mut g, p, d, hw, &kerb_cols);
                }
            }
        }
        if let Some(side) = ring_out {
            // Round the ring: lamps on the outer pavement, nothing else.
            let len = path_length(&walk);
            let mut s = 6.0;
            while s < len - 4.0 {
                let (p, d) = point_along(&walk, s);
                let q = p + Vec2::new(-d.y, d.x) * side * (hw + 0.95);
                if !junction::on_carriageway(roads, heights, inp.inner, q, run_z(ri, q), 0.05) {
                    lamp_post_world(&mut g, q, 7.0, pal.street_lamp);
                }
                s += 28.0;
            }
            continue;
        }
        // Furniture by arc length along the pavement, none of it standing
        // in another street's lanes.
        let len = path_length(&walk);
        let lamp_off = hw + 0.95;
        let back_off = hw + SIDEWALK - 0.7;
        let at = |s: f32, side: f32, off: f32| {
            let (p, d) = point_along(&walk, s);
            let right = Vec2::new(-d.y, d.x);
            (p + right * side * off, d, right)
        };
        let free = |p: Vec2| !junction::on_carriageway(roads, heights, inp.inner, p, run_z(ri, p), 0.05);
        // On that side's pavement, not out on a junction's asphalt where
        // the kerb starts further along (a fork's wedge).
        let run_len = path_length(pts);
        let kerbed = |s: f32, side: f32| {
            let (e0, e1) = kerb_ends[(side > 0.0) as usize];
            t0 + s >= e0 + 0.8 && t0 + s <= run_len - e1 - 0.8
        };
        let mut lamp_ts: Vec<(f32, f32)> = Vec::new();
        let mut s = 6.0;
        let mut side = 1.0;
        while s < len - 4.0 {
            let (p, _, _) = at(s, side, lamp_off);
            if free(p) && kerbed(s, side) {
                lamp_post_world(&mut g, p, 7.0, pal.street_lamp);
                lamp_ts.push((s, side));
            }
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
                    if !free(p) || !kerbed(s, side) {
                        continue;
                    }
                    g.part(Concrete).boxed(Vec3::new(p.x, KERB_H + 0.2, p.y), Vec3::new(1.3, 0.4, 1.3), d.y.atan2(d.x), rgba(mul(pal.paving, 1.1)));
                    crate::climate::climate_tree(&mut g, pal, p, rng.random_range(3.0..4.5), rng.random_range(2.8..4.0), mul(pal.foliage, rng.random_range(0.85..1.15)));
                }
                s += 13.0 + rng.random_range(0.0..3.0);
            }
        }
        if lv_mid < 0.4 {
            let mut prev: Option<Vec3> = None;
            let mut s = 2.0;
            while s < len - 1.0 {
                let (p, d, right) = at(s, -1.0, back_off);
                // Wooden poles stand in the ground, not on a bridge's or
                // viaduct's deck; the wires span it to the next pole.
                let on_deck = run.nodes.windows(2).filter_map(|w| segment_between(roads, w[0], w[1])).any(|sid| {
                    let sg = roads.segment(sid);
                    let (a, b) = roads.segment_points(sid);
                    (sg.bridge || sg.viaduct) && nd_sim::roads::project_on_segment(p, a, b).1.distance(p) < hw + SIDEWALK + 0.5
                });
                if !free(p) || !kerbed(s, -1.0) || on_deck {
                    s += 32.0;
                    continue;
                }
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
            let pick = rng.random_range(0..6);
            if !free(p) || !free(at(s, side, back_off).0) || !kerbed(s, side) {
                s += 22.0 + rng.random_range(0.0..20.0);
                continue;
            }
            match pick {
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
        if !inp.layer.keeps(tunnel_node(roads, NodeId(i as u32))) {
            continue;
        }
        let alive: Vec<SegmentId> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
        let paths_here = node.segments.iter().filter(|&&s| roads.segment(s).alive && roads.segment(s).kind == RoadKind::Path).count();
        let rails_here: Vec<SegmentId> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_rail()).collect();
        if !alive.is_empty() && !rails_here.is_empty() {
            level_crossing(&mut g, pal, roads, NodeId(i as u32), node.pos, &alive, &rails_here);
            continue;
        }
        if alive.len() == 2 && paths_here > 0 {
            // A path crossing a street: zebra across the carriageway (none
            // where a footbridge replaced it).
            if crosswalk_closed(inp.closed, node.pos) {
                continue;
            }
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
                g.part(Paint).ground_strip(&[c0 + right * s, c1 + right * s], 0.5, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 0.9)));
                s += 1.0;
            }
            continue;
        }
        if alive.len() == 2 {
            // Where a road carries on with fewer or more lanes (a ramp onto
            // a highway's end, a street that widens), the carriageway
            // narrows over a taper instead of stepping at the joint.
            if let Some((_, ri, path, hw0, hw1)) = tapers.iter().find(|t| t.0 == NodeId(i as u32)) {
                let (ri, hw0, hw1) = (*ri, *hw0, *hw1);
                let from = marks(&g);
                let motorway = alive.iter().any(|&s| roads.segment(s).is_motorway());
                lane_taper(&mut g, path, hw0, hw1, asphalt_c, edge_c, motorway.then(|| rgba(mul(pal.concrete(0), 0.8))));
                if !lifts.is_empty() {
                    // On the narrower run's profile (the node loop adds the
                    // node's height to all it draws).
                    let z0 = node_z(NodeId(i as u32));
                    lift_since(&mut g, &from, |p| run_z(ri, p) - z0);
                }
            }
            continue;
        }
        if alive.is_empty() {
            continue;
        }
        // A ramp meeting a highway: a taper and a hatched gore instead of a
        // junction pad, crossings and stop lines.
        if let Some(ramp) = gore_ramp(roads, NodeId(i as u32)) {
            // Bright paint, as the lane arrows: the hatching is the one
            // marking that tells a driver where not to be.
            let (v, ix) = (g.get(Asphalt).positions.len(), g.get(Asphalt).indices.len());
            ramp_gore(&mut g, roads, NodeId(i as u32), ramp, asphalt_c, rgba(mul(pal.lane_marker, 1.25)));
            // In the same asphalt as the carriageway it widens out of (a
            // highway out of town is worn; the gore was a pale patch on it).
            let carriageway = gore_lines(roads, NodeId(i as u32), ramp).and_then(|_| {
                node.segments.iter().copied().find(|&s| s != ramp && roads.segment(s).alive && roads.segment(s).is_highway())
            });
            if carriageway.and_then(|s| arm_run.get(&(i as u32, s.0))).is_some_and(|&ri| run_worn[ri]) {
                weather_tail(&mut g, vec![(Asphalt, v, ix)]);
            }
            continue;
        }
        let nid = NodeId(i as u32);
        let Some(arms) = pad_mouths.get(&(i as u32)) else { continue };
        // The pad: the polygon through the arms' mouths and the corners'
        // kerb lines (`junction::pad_outline`), each point at the height of
        // the road beside it (the node loop adds the node's own), worn like
        // the corners at a cheap junction.
        let node_corners = corners.by_node.get(&(i as u32));
        let corner_x = |sa: SegmentId, sb: SegmentId| node_corners.into_iter().flatten().find(|c| c.arms[0].2 == sa && c.arms[1].2 == sb && c.turn < 0.0 && !c.apron.is_empty()).map(|c| c.apron[0]);
        let outline = junction::pad_outline_with(node.pos, arms, &corner_x);
        if outline.len() >= 3 {
            let z0 = node_z(nid);
            let local = |pp: &junction::PadPoint| {
                let rz = |k: usize| arm_run.get(&(i as u32, arms[k].seg.0)).map_or(z0, |&ri| run_z(ri, pp.p));
                if lifts.is_empty() {
                    0.0
                } else if pp.arms[0] == pp.arms[1] {
                    rz(pp.arms[0]) - z0
                } else {
                    rz(pp.arms[0]) + rz(pp.arms[1]) - 2.0 * z0
                }
            };
            let ring: Vec<Vec3> = outline.iter().map(|pp| Vec3::new(pp.p.x, ASPHALT_Y + 0.01 + local(pp), pp.p.y)).collect();
            let worn = inp.land_value.at(node.pos) < 0.35;
            let (v, ix) = (g.get(Asphalt).positions.len(), g.get(Asphalt).indices.len());
            g.part(Asphalt).ground_fan(Vec3::new(node.pos.x, ASPHALT_Y + 0.01, node.pos.y), &ring, asphalt_c);
            if worn {
                weather_tail(&mut g, vec![(Asphalt, v, ix)]);
            }
        }
        let on_ring = roads.ring_of_node(nid).is_some();
        let r = net.node_radius.get(i).copied().unwrap_or(4.0) + 0.3;
        for m in arms {
            let sid = m.seg;
            let seg = roads.segment(sid);
            let d = m.dir;
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
            // The zebra where the walk graph's crosswalk is (and the kerbs
            // drop): it used to be painted at the junction's radius whatever
            // the set-back, so at a sharp corner the paint and the curb cut
            // were metres apart and two arms' zebras overlapped.
            // [footbridge] None where a footbridge replaced the crosswalk:
            // the stop line moves up to the junction's mouth.
            let stop = if junction::has_crosswalk(roads, nid, sid) && !crosswalk_closed(inp.closed, node.pos + d * nd_sim::walk::crosswalk_setback(roads, nid, sid)) {
                let mid = nd_sim::walk::crosswalk_setback(roads, nid, sid);
                let half = nd_sim::walk::CROSSWALK_HALF;
                let (c0, c1) = (node.pos + d * (mid - half), node.pos + d * (mid + half));
                let mut s = -hw + SHOULDER + 0.4;
                while s < hw - SHOULDER - 0.2 {
                    g.part(Paint).ground_strip(&[c0 + right * s, c1 + right * s], 0.5, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 0.9)));
                    s += 1.0;
                }
                mid + half + 0.5
            } else {
                m.at + 0.5
            };
            let incoming = if seg.a.0 as usize == i { seg.lanes_bwd } else { seg.lanes_fwd } as f32;
            if incoming > 0.0 {
                let w = incoming * LANE_WIDTH;
                let centre = node.pos + d * stop - right * (w * 0.5);
                g.part(Paint).ground_strip(&[centre - right * w * 0.5, centre + right * w * 0.5], 0.4, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 1.1)));
            }
        }
    }

    if let Some((from, z)) = node_from.take() {
        if z != 0.0 {
            lift_since(&mut g, &from, |_| z);
        }
    }

    // Junction corners: a rounded kerb from one arm's pavement round to the
    // next, the paving behind it out to where the arms' pavements end, and
    // asphalt out to where the kerb lines would have met, so no bare ground
    // shows between the arms. The kerb drops where the arms' crosswalks
    // cross it. Each corner is set on the heights of the two runs it joins,
    // blended along it; a cheap junction's corners are worn like its streets.
    for (&node, list) in corners.by_node.iter().filter(|_| inp.layer.keeps(false)) {
        let nid = NodeId(node);
        let centre = roads.node(nid).pos;
        let worn: Vec<(Surface, usize, usize)> = [Asphalt, Paving, Concrete].iter().map(|s| (*s, g.get(*s).positions.len(), g.get(*s).indices.len())).collect();
        for corner in list {
            let from = marks(&g);
            let corner_cuts: Vec<Cut> = corner.arms.iter().flat_map(|a| cuts.get(&a.2 .0).into_iter().flatten().copied()).collect();
            kerb_ribbon(&mut g, &corner.kerb, Some(&corner.normals), -1.0, CORNER_RIBBON, false, corner.stub, &corner_cuts, &kerb_cols);
            if corner.apron.len() >= 3 {
                g.part(Asphalt).ground_polygon(&corner.apron, ASPHALT_Y + 0.01, asphalt_c);
            }

            if corner.fill.len() >= 3 {
                // 3 mm over the pavements it overlaps (theirs at KERB_H -
                // 0.01, with other UVs): one surface wins, no flicker.
                g.part(Paving).ground_polygon(&corner.fill, CORNER_FILL_Y, paving_c);
                let mut edges: Vec<(Vec2, Vec2, Option<Vec2>)> = corner.rim.windows(2).map(|w| (w[0], w[1], None)).collect();
                if corner.stub && !corner.rim.is_empty() {
                    // Where the paving stops beside a link with none: its
                    // two ends (one of them hidden under the other arm's).
                    let (k0, k1) = (corner.kerb[0], corner.kerb[corner.kerb.len() - 1]);
                    let back_last = corner.fill[corner.fill.len() - corner.rim.len() - 1];
                    edges.push((back_last, corner.rim[0], Some(k1 - corner.kerb[corner.kerb.len() - 2])));
                    edges.push((corner.rim[corner.rim.len() - 1], corner.fill[0], Some(k0 - corner.kerb[1])));
                }
                for (p, q, facing) in edges {
                    let n = Vec2::new(-(q - p).y, (q - p).x);
                    let towards = facing.unwrap_or((p + q) * 0.5 - centre);
                    let n = if n.dot(towards) < 0.0 { -n } else { n };
                    let y = CORNER_FILL_Y;
                    g.part(Paving).quad_facing([Vec3::new(p.x, 0.0, p.y), Vec3::new(q.x, 0.0, q.y), Vec3::new(q.x, y, q.y), Vec3::new(p.x, y, p.y)], Vec3::new(n.x, 0.0, n.y), paving_c);
                }
            }
            // Bollards on the corner of a junction proper, behind the kerb
            // where turning lorries cut it.
            if degree(roads, nid) >= 3 && corner.turn < -0.5 {
                let len = path_length(&corner.kerb);
                for ds in [-0.7f32, 0.7] {
                    let (p, d) = point_along(&corner.kerb, len * 0.5 + ds);
                    bollard_world(&mut g, p + Vec2::new(d.y, -d.x) * 0.55, mul(pal.steel, 0.7));
                }
            }
            if !lifts.is_empty() {
                // Each run's profile is the node's height at the junction and
                // rises along the run, so the two together are the height of
                // the road beside either arm (the other contributing only
                // the node's), and a plane through both slopes between.
                let (ra, rb) = (corner.arms[0].0, corner.arms[1].0);
                let z0 = node_z(nid);
                lift_since(&mut g, &from, |p| run_z(ra, p) + run_z(rb, p) - z0);
            }
        }
        if inp.land_value.at(centre) < 0.35 {
            weather_tail(&mut g, worn);
        }
    }

    // [pedestrians] Mid-block zebras on long blocks, with a beacon post at
    // each kerb.
    for (sid, t) in nd_sim::walk::midblock_crossings(roads) {
        let seg = roads.segment(sid);
        // Each layer draws only its own roads' crossings: these used to be
        // drawn in every layer, so the underground view showed zebras and
        // beacons floating over the surface streets.
        if !inp.layer.keeps(seg.tunnel) {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let d = (b - a).normalize_or_zero();
        let right = Vec2::new(-d.y, d.x);
        let hw = seg.half_width();
        let at = a.lerp(b, t);
        if crosswalk_closed(inp.closed, at) {
            continue;
        }
        // [elevation] At the road's own height along the segment.
        let from = marks(&g);
        let z = nd_sim::landscape::way_height(roads, heights, inp.inner, sid, t);
        let (c0, c1) = (at - d * nd_sim::walk::CROSSWALK_HALF, at + d * nd_sim::walk::CROSSWALK_HALF);
        let mut s = -hw + SHOULDER + 0.4;
        while s < hw - SHOULDER - 0.2 {
            g.part(Paint).ground_strip(&[c0 + right * s, c1 + right * s], 0.5, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 0.9)));
            s += 1.0;
        }
        for side in [-1.0f32, 1.0] {
            let p = at + right * side * (hw + 0.45) + d * (nd_sim::walk::CROSSWALK_HALF + 0.3);
            g.part(Matte).cylinder(p, 0.06, 0.05, 0.0, 2.6, 6, rgba(CHARCOAL));
            g.part(Glow).boxed(Vec3::new(p.x, 2.75, p.y), Vec3::splat(0.3), 0.0, rgba([1.0, 0.62, 0.15]));
        }
        if z != 0.0 {
            lift_since(&mut g, &from, |_| z);
        }
    }

    // [pedestrians] Walk / don't walk heads on signalised crosswalks: a
    // short pole at each kerb with a head facing across.
    for post in ped_signal_posts_at(roads, net, &corners).into_iter().filter(|p| !crosswalk_closed(inp.closed, p.crosswalk)) {
        // The junction the crosswalk belongs to decides the layer and the
        // height (not the nearest node, which may be another road's).
        let Some(node) = net.intersections.get(post.inter).map(|i| i.node) else { continue };
        if !inp.layer.keeps(tunnel_node(roads, node)) {
            continue;
        }
        let from = marks(&g);
        let z = node_z(node);
        g.part(Matte).cylinder(post.pole, 0.07, 0.06, 0.0, PED_HEAD_Y + 0.4, 6, rgba(CHARCOAL));
        let f = Vec2::from_angle(post.heading);
        let head = Vec3::new(post.pole.x + f.x * 0.12, PED_HEAD_Y, post.pole.y + f.y * 0.12);
        g.part(Matte).boxed(head, Vec3::new(0.34, 0.62, 0.34), 0.0, rgba(DARK));
        if z != 0.0 {
            lift_since(&mut g, &from, |_| z);
        }
    }

    // Signal posts: pole at the kerb, arm over the approach, one head per lane.
    for post in signal_groups(roads, net, &corners) {
        // Layer and height from the junction the approach feeds: two
        // nearest-node scans per post were O(nodes) each, and a node with
        // no live roads counted as underground (`all` of nothing).
        if !inp.layer.keeps(roads.nodes.get(post.node.0 as usize).is_some_and(|_| tunnel_node(roads, post.node))) {
            continue;
        }
        let from = marks(&g);
        let base = post.pole;
        // [interchange] The arm keeps its clearance over the lanes it hangs
        // over: on an approach climbing to the junction from below (or
        // falling to it) the road under the heads stands higher than the
        // junction the post is set on, and the arm hung 3.2 m over it.
        let z = node_z(post.node);
        let rise = if heights.is_empty() {
            0.0
        } else {
            let (sa, sb) = roads.segment_points(post.seg);
            let len = sa.distance(sb).max(1e-3);
            post.heads.iter().chain(std::iter::once(&post.leftmost)).map(|&h| {
                let t = nd_sim::roads::project_on_segment(h, sa, sb).0;
                nd_sim::landscape::way_height(roads, heights, inp.inner, post.seg, t) - z
            }).fold(0.0f32, f32::max).min(len)
        };
        let pole_h = 5.6 + rise;
        g.part(Matte).cylinder(base, 0.16, 0.12, 0.0, pole_h, 8, rgba(CHARCOAL));
        let a = Vec3::new(base.x, pole_h - 0.2, base.y);
        let b = Vec3::new(post.leftmost.x, pole_h - 0.2, post.leftmost.y);
        g.part(Matte).beam(a, b, 0.18, rgba(CHARCOAL));
        for lamp in &post.heads {
            let head = Vec3::new(lamp.x, 4.75 + rise, lamp.y);
            g.part(Matte).boxed(head, Vec3::new(0.35, 1.1, 0.4), -post.heading, rgba(DARK));
            g.part(Matte).beam(Vec3::new(lamp.x, pole_h - 0.2, lamp.y), head + Vec3::Y * 0.55, 0.08, rgba(CHARCOAL));
        }
        if z != 0.0 {
            lift_since(&mut g, &from, |_| z);
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
            g.part(Paint).path_strip(&[a - d * 0.6, b + d * 0.6], 0.0, w0 + 0.5, MARK_Y, rgba(mul(pal.lane_marker, 0.5)));
        }
    }
    let mut s = -hw + SHOULDER + 0.4;
    while s < hw - SHOULDER - 0.2 {
        if (s - divide).abs() > 0.6 || outgoing == 0 || incoming == 0 {
            g.part(Paint).ground_strip(&[at + d * z0 + right * s, at + d * z1 + right * s], 0.5, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 0.9)));
        }
        s += 1.0;
    }
    if incoming > 0 {
        let (l0, l1) = (-(hw - SHOULDER), divide);
        let mut x = l0 + 0.3;
        while x + 0.6 < l1 {
            let p = at + d * (r + 0.9) + right * x;
            g.part(Paint).ground_strip(&[p, p + right * 0.6], 0.35, MARK_Y + 0.003, rgba(mul(pal.lane_marker, 1.1)));
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
fn railway(g: &mut Geometry, pal: &Palette, roads: &RoadNetwork, run: &Run, points: &[Vec2]) {
    // Stopped at a level crossing's edge, where `level_crossing` lays the
    // rails into the road: the ballast, sleepers and rails used to run on
    // over the asphalt and the kerbs, standing 0.3 m proud of the road.
    let (t0, t1) = (crossing_reach(roads, run.start_node, points, true, RAIL_HALF_WIDTH), crossing_reach(roads, run.end_node, points, false, RAIL_HALF_WIDTH));
    let pts = if t0 == 0.0 && t1 == 0.0 { points.to_vec() } else { trim(points, t0, t1) };
    railway_track(g, pal, &pts, RAIL_HALF_WIDTH * 2.0);
}

/// How far along a path or track from its end at `node` it stays back for
/// the streets crossing there: out to the back of their pavements, measured
/// along it (further at a slant). Zero where no street meets it.
fn crossing_reach(roads: &RoadNetwork, node: NodeId, points: &[Vec2], start: bool, half: f32) -> f32 {
    if roads.street_degree(node) == 0 || points.len() < 2 {
        return 0.0;
    }
    let n = points.len();
    let d = if start { points[1] - points[0] } else { points[n - 2] - points[n - 1] }.normalize_or_zero();
    roads
        .node(node)
        .segments
        .iter()
        .filter(|&&s| roads.segment(s).alive && roads.segment(s).is_street())
        .map(|&s| {
            let (a, b) = roads.segment_points(s);
            let along = (b - a).normalize_or_zero();
            let sin = d.perp_dot(along).abs().max(0.35);
            // Its own width's corners reach further at a slant.
            (roads.segment(s).half_width() + SIDEWALK + half * d.dot(along).abs()) / sin
        })
        .fold(0.0, f32::max)
}

/// Where track crosses a street: planks between the rails over the
/// carriageway, crossbuck signs and barrier posts on both sides.
fn level_crossing(g: &mut Geometry, pal: &Palette, roads: &RoadNetwork, node: NodeId, at: Vec2, streets: &[SegmentId], rails: &[SegmentId]) {
    let Some(&street) = streets.first() else { return };
    let Some(&rail) = rails.first() else { return };
    let (sa, sb) = roads.segment_points(street);
    let sd = (sb - sa).normalize_or_zero();
    let s_right = Vec2::new(-sd.y, sd.x);
    let (ra, rb) = roads.segment_points(rail);
    let rd = (ra - rb).normalize_or_zero();
    let hw = roads.segment(street).half_width();
    // The rails laid flush into the road: a crossing panel between and
    // beside them over the carriageway, cut square to the kerb lines (the
    // planks used to float 15 cm over the asphalt and run out over the
    // pavements), and the two rails over it and across the pavements at
    // their height. The track itself stops at the back of the pavements
    // (`railway`).
    let rn = Vec2::new(-rd.y, rd.x);
    let (cu, cv) = (rd.dot(s_right), rn.dot(s_right));
    if cu.abs() > 0.2 {
        // The point on the rail's line `v` across it whose offset from the
        // street's centre line is `lat`.
        let at_lat = |lat: f32, v: f32| at + rd * ((lat - v * cv) / cu) + rn * v;
        let half = RAIL_HALF_WIDTH.min(1.6);
        let panel = [at_lat(-hw, -half), at_lat(hw, -half), at_lat(hw, half), at_lat(-hw, half)];
        g.part(Wood).ground_polygon(&panel, MARK_Y + 0.001, rgba(mul(pal.wood, 0.45)));
        for v in [-0.72f32, 0.72] {
            g.part(Metal).ground_strip(&[at_lat(-hw, v), at_lat(hw, v)], 0.08, MARK_Y + 0.006, rgba(mul(pal.steel, 0.75)));
            for side in [-1.0f32, 1.0] {
                g.part(Metal).ground_strip(&[at_lat(side * hw, v), at_lat(side * (hw + SIDEWALK), v)], 0.08, KERB_H - 0.004, rgba(mul(pal.steel, 0.75)));
            }
        }
    }
    // [rail-crossing] Stop lines across the arriving lanes, clear of the
    // track (the lanes end there: `nd_sim::level_crossing::stop_radius`).
    // The posts, lamps and barriers are the app's (`crate::crossing`).
    if let Some(c) = nd_sim::level_crossing::at(roads, node) {
        for a in c.arms.iter().filter(|a| !a.path && a.lanes_in > 0) {
            let h = a.heading();
            let right = Vec2::new(-h.y, h.x);
            let total = (a.lanes_in + a.lanes_out) as f32;
            let p = at + a.away * (a.stop + 1.0);
            let (from, to) = (-total * 0.5 * LANE_WIDTH + a.lanes_out as f32 * LANE_WIDTH, total * 0.5 * LANE_WIDTH);
            let from = if a.lanes_out > 0 { from + 0.15 } else { from };
            g.part(Paint).ground_strip(&[p + right * from, p + right * to], 0.45, MARK_Y + 0.003, rgba(mul(pal.lane_marker, 1.0)));
        }
    }
}

/// A path for walkers and bikes: paving with edge stones, low lamps, the odd
/// bench and bin, and bollards where it meets a street so cars stay out.
fn footpath(g: &mut Geometry, pal: &Palette, roads: &RoadNetwork, run: &Run, points: &[Vec2], rng: &mut impl Rng) {
    // Where the path meets a street, stop at the back of that street's
    // pavement and flare out onto it, instead of running under the kerb.
    let mut points = points.to_vec();
    for (end, node) in [(true, run.start_node), (false, run.end_node)] {
        if roads.street_degree(node) == 0 || points.len() < 2 {
            continue;
        }
        let back = crossing_reach(roads, node, &points, end, PATH_HALF_WIDTH);
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
    g.part(Paint).path_strip(pts, 0.0, 0.08, MARK_Y, rgba(mul(pal.lane_divider, 0.6)));
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
    /// The junction the approach feeds.
    node: NodeId,
    /// [interchange] The approach.
    seg: SegmentId,
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
fn signal_groups(roads: &RoadNetwork, net: &LaneNetwork, corners: &Corners) -> Vec<SignalGroup> {
    let feeders = feeders(net);
    let mut arms: HashMap<u32, Vec<Mouth>> = HashMap::new();
    let mut groups: BTreeMap<(u32, bool), (NodeId, Vec<(u8, Vec2, Vec2)>)> = BTreeMap::new();
    for inter in &net.intersections {
        if !inter.signalized {
            continue;
        }
        for &(conn, _) in &inter.connectors {
            for &lid in feeders.get(&conn).map(|v| v.as_slice()).unwrap_or(&[]) {
                let lane = &net.lanes[lid as usize];
                if let LaneKind::Road { segment, forward, index } = lane.kind {
                    let back = junction::extra_stop_back(roads, net, inter.node, segment);
                    let (p, d) = lane.point_at((lane.length - 0.6 - back).max(0.0));
                    let (_, entry) = groups.entry((segment.0, forward)).or_insert_with(|| (inter.node, Vec::new()));
                    if !entry.iter().any(|(i, _, _)| *i == index) {
                        entry.push((index, p, d));
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for ((seg, _), (node, lanes)) in groups.into_iter() {
        let Some((_, _, d)) = lanes.first() else { continue };
        let d = *d;
        let right = Vec2::new(-d.y, d.x);
        let rightmost = lanes.iter().max_by_key(|(i, _, _)| *i).unwrap();
        let leftmost = lanes.iter().min_by_key(|(i, _, _)| *i).unwrap();
        // On the pavement: behind the corner's kerb where the corner is
        // rounded off past it.
        let pole = behind_kerb(rightmost.1 + right * (LANE_WIDTH * 0.5 + SHOULDER + 0.45), corners.by_node.get(&node.0), 0.45);
        let pole = if pad_node(roads, node) { junction::place_pole(roads.node(node).pos, arms.entry(node.0).or_insert_with(|| mouths(roads, net, node)), SegmentId(seg), pole, 0.45) } else { pole };
        out.push(SignalGroup { node, seg: SegmentId(seg), pole, leftmost: leftmost.1 - right * (LANE_WIDTH * 0.2), heads: lanes.iter().map(|(_, p, _)| *p).collect(), heading: d.y.atan2(d.x) });
    }
    out
}

/// [pedestrians] Walk lamps for every arm of every signalised junction
/// (none across a highway), two per crosswalk, in junction and arm order.
/// The crosswalk sits where `nd_sim::walk` puts it, so the lamps stand at
/// the kerbs walkers wait at.
pub fn ped_signal_posts(roads: &RoadNetwork, net: &LaneNetwork) -> Vec<PedSignalPost> {
    ped_signal_posts_at(roads, net, &junction_corners(roads, net, &build_runs(roads)))
}

fn ped_signal_posts_at(roads: &RoadNetwork, net: &LaneNetwork, corners: &Corners) -> Vec<PedSignalPost> {
    let mut out = Vec::new();
    for (k, inter) in net.intersections.iter().enumerate() {
        if !inter.signalized || roads.nodes.len() <= inter.node.0 as usize {
            continue;
        }
        let node = roads.node(inter.node);
        let arms: Vec<SegmentId> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
        let mouth_list = if pad_node(roads, inter.node) { mouths(roads, net, inter.node) } else { Vec::new() };
        for (ap, &sid) in arms.iter().enumerate() {
            let seg = roads.segment(sid);
            // No walk lights where nobody crosses (a ramp, a tunnel: the
            // walk graph has no crosswalk there either).
            if !junction::has_crosswalk(roads, inter.node, sid) {
                continue;
            }
            let (a, b) = roads.segment_points(sid);
            let away = (if seg.a == inter.node { b - a } else { a - b }).normalize_or_zero();
            let side = Vec2::new(-away.y, away.x);
            let mid = node.pos + away * nd_sim::walk::crosswalk_setback(roads, inter.node, sid);
            let hw = seg.half_width();
            for sgn in [-1.0f32, 1.0] {
                // Just outside the zebra, on the kerb, facing the far kerb.
                let pole = behind_kerb(mid + side * sgn * (hw + 0.35) + away * (nd_sim::walk::CROSSWALK_HALF + 0.35), corners.by_node.get(&inter.node.0), 0.35);
                let pole = junction::place_pole(node.pos, &mouth_list, sid, pole, 0.35);
                let facing = -side * sgn;
                let lamp = Vec3::new(pole.x + facing.x * 0.3, PED_HEAD_Y, pole.y + facing.y * 0.3);
                out.push(PedSignalPost { inter: k, approach: ap, crosswalk: mid, pole, lamp, heading: facing.y.atan2(facing.x) });
            }
        }
    }
    out
}

/// Lamp positions for the renderer, one per lane feeding a signal, in lane
/// order.
pub fn signal_posts(roads: &RoadNetwork, net: &LaneNetwork) -> Vec<SignalPost> {
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
            let back = match lane.kind {
                LaneKind::Road { segment, .. } => junction::extra_stop_back(roads, net, lane.end_node, segment),
                _ => 0.0,
            };
            let (p, d) = lane.point_at((lane.length - 0.6 - back).max(0.0));
            SignalPost { lane: lid, lamp: Vec3::new(p.x - d.x * 0.24, 4.95, p.y - d.y * 0.24), heading: d.y.atan2(d.x) }
        })
        .collect()
}

// ── kerbs, corners and curb cuts ────────────────────────────────────────────

/// Kerb width; its road-side face is at the carriageway's half width.
const KERB_W: f32 = 0.3;
/// Top of a dropped kerb at a crossing: just proud of the asphalt.
const KERB_LOW: f32 = ASPHALT_Y + 0.025;
const KERB_DROP: f32 = KERB_H - KERB_LOW;
/// Along the kerb, the sloped flare either side of a dropped kerb.
const CUT_FLARE: f32 = 0.8;
/// Behind a dropped kerb: the low landing the tactile pad lies on, then the
/// ramp back up to the pavement.
const CUT_LANDING: f32 = 0.9;
const CUT_RAMP: f32 = 0.8;
/// Depth of a tactile pad (the real ones are 0.8 m deep at a crossing).
const TACTILE_DEPTH: f32 = 0.8;
/// How deep a corner's kerb ribbon reaches into the pavement: to the top of
/// a curb cut's ramp. The flat paving fill of the corner begins there.
const CORNER_RIBBON: f32 = KERB_W + CUT_LANDING + CUT_RAMP;
/// The paving that fills a junction corner behind its kerb ribbon: a layer
/// over the arms' pavements (KERB_H - 0.01) where it overlaps them.
const CORNER_FILL_Y: f32 = KERB_H - 0.007;

/// Where a crosswalk meets the kerbs of the street it crosses: a band
/// `half` either side of `mid` along the street, as far out to either side
/// as the street's pavements reach.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cut {
    /// On the street's centre line, in the middle of the crosswalk.
    pub mid: Vec2,
    /// Along the street.
    pub along: Vec2,
    pub half: f32,
    /// How far across the street (from its centre line) the cut applies.
    pub reach: f32,
}

impl Cut {
    fn u(&self, p: Vec2) -> f32 {
        (p - self.mid).dot(self.along)
    }

    fn near(&self, p: Vec2) -> bool {
        (p - self.mid).perp_dot(self.along).abs() <= self.reach
    }

    /// How far the kerb at `p` is dropped: 1 across the crosswalk, falling
    /// to 0 over the flares.
    fn drop(&self, p: Vec2) -> f32 {
        if !self.near(p) {
            return 0.0;
        }
        let u = self.u(p).abs();
        if u <= self.half {
            1.0
        } else {
            (1.0 - (u - self.half) / CUT_FLARE).max(0.0)
        }
    }

    fn on_pad(&self, p: Vec2) -> bool {
        self.near(p) && self.u(p).abs() <= self.half + 1e-3
    }
}

/// The segment joining two nodes, if one is alive.
fn segment_between(roads: &RoadNetwork, na: NodeId, nb: NodeId) -> Option<SegmentId> {
    roads.node(na).segments.iter().copied().find(|&s| {
        let sg = roads.segment(s);
        sg.alive && ((sg.a == na && sg.b == nb) || (sg.a == nb && sg.b == na))
    })
}

/// Every place a marked crosswalk meets the kerbs, by the segment crossed:
/// a junction's crosswalk across each arm (where `nd_sim::walk` puts it and
/// the paint goes), mid-block zebras, and the zebra where a footpath
/// crosses a street. The kerbs drop wherever these meet them.
pub(crate) fn crossing_cuts(roads: &RoadNetwork) -> HashMap<u32, Vec<Cut>> {
    let mut out: HashMap<u32, Vec<Cut>> = HashMap::new();
    let mut add = |sid: SegmentId, mid: Vec2, along: Vec2, half: f32| {
        let reach = roads.segment(sid).half_width() + SIDEWALK + 0.5;
        out.entry(sid.0).or_default().push(Cut { mid, along, half, reach });
    };
    for (i, node) in roads.nodes.iter().enumerate() {
        let nid = NodeId(i as u32);
        let streets: Vec<SegmentId> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
        if streets.is_empty() || node.segments.iter().any(|&s| roads.segment(s).alive && roads.segment(s).is_rail()) {
            continue;
        }
        if streets.len() == 2 {
            // A footpath over a street: the zebra across it at the node.
            if node.segments.iter().any(|&s| roads.segment(s).alive && roads.segment(s).kind == RoadKind::Path) {
                let (a, b) = roads.segment_points(streets[0]);
                let d = (b - a).normalize_or_zero();
                for &s in &streets {
                    add(s, node.pos, d, 1.3);
                }
            }
            continue;
        }
        if streets.len() < 3 {
            continue;
        }
        // Nobody crosses a ring's own lanes on a marked crosswalk, nor a
        // highway or a tunnel (`nd_sim::walk::WalkGraph::junction`).
        let on_ring = roads.ring_of_node(nid).is_some();
        for &sid in &streets {
            let seg = roads.segment(sid);
            if seg.fronts_nothing() || (on_ring && roads.ring_of_segment(sid).is_some()) {
                continue;
            }
            let (a, b) = roads.segment_points(sid);
            let away = (if seg.a == nid { b - a } else { a - b }).normalize_or_zero();
            add(sid, node.pos + away * nd_sim::walk::crosswalk_setback(roads, nid, sid), away, nd_sim::walk::CROSSWALK_HALF);
        }
    }
    for (sid, t) in nd_sim::walk::midblock_crossings(roads) {
        let (a, b) = roads.segment_points(sid);
        add(sid, a.lerp(b, t), (b - a).normalize_or_zero(), nd_sim::walk::CROSSWALK_HALF);
    }
    out
}

/// The colours a kerb ribbon is drawn in.
struct KerbColours {
    kerb: [f32; 4],
    paving: [f32; 4],
    tactile: [f32; 4],
}

/// A kerb and the pavement behind it along `face` (the kerb's road-side
/// edge), with the pavement on the `out` side (+1 right of travel, as
/// `offset_path` counts it) and `width` deep from the face. `back` closes
/// the pavement's far edge and `caps` its two ends. `normals`, if given,
/// are the exact directions into the pavement at each point of `face` (a
/// rounded corner's: mitred offsets fold over on its short chords).
///
/// Wherever a crosswalk in `cuts` meets it, the kerb drops to just above
/// the road across the crosswalk's width with a sloped flare either side;
/// behind it the pavement dips to a landing and ramps back up, and a
/// tactile pad lies on the landing. Heights are linear in position along
/// each straight piece of the kerb, so a point wherever a cut's band or
/// flare begins or ends is all the shape needs: a plain stretch is four
/// quads a piece, a curb cut a few dozen triangles more.
#[allow(clippy::too_many_arguments)]
fn kerb_ribbon(g: &mut Geometry, face: &[Vec2], normals: Option<&[Vec2]>, out: f32, width: f32, back: bool, caps: bool, cuts: &[Cut], c: &KerbColours) {
    if face.len() < 2 || normals.is_some_and(|n| n.len() != face.len()) {
        return;
    }
    let mut pts: Vec<Vec2> = vec![face[0]];
    let mut nrm: Vec<Vec2> = normals.map(|n| vec![n[0]]).unwrap_or_default();
    for (wi, w) in face.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        let (na, nb) = normals.map_or((Vec2::ZERO, Vec2::ZERO), |n| (n[wi], n[wi + 1]));
        let mut ts: Vec<f32> = Vec::new();
        for k in cuts {
            if !(k.near(a) || k.near(b)) {
                continue;
            }
            let (ua, ub) = (k.u(a), k.u(b));
            if (ub - ua).abs() < 1e-5 {
                continue;
            }
            for thr in [-k.half - CUT_FLARE, -k.half, k.half, k.half + CUT_FLARE] {
                let t = (thr - ua) / (ub - ua);
                if t > 1e-3 && t < 1.0 - 1e-3 {
                    ts.push(t);
                }
            }
        }
        ts.sort_by(f32::total_cmp);
        let mut last = 0.0;
        for t in ts {
            if t - last > 1e-3 {
                pts.push(a.lerp(b, t));
                if normals.is_some() {
                    nrm.push(na.lerp(nb, t).normalize_or_zero());
                }
                last = t;
            }
        }
        if b.distance(*pts.last().unwrap()) > 1e-3 {
            pts.push(b);
            if normals.is_some() {
                nrm.push(nb);
            }
        }
    }
    let n = pts.len();
    if n < 2 {
        return;
    }
    let f: Vec<f32> = pts.iter().map(|&p| cuts.iter().map(|k| k.drop(p)).fold(0.0, f32::max)).collect();
    let pad: Vec<Option<usize>> = pts.iter().map(|&p| cuts.iter().position(|k| k.on_pad(p))).collect();
    // Stations across: kerb face, kerb back, end of the landing, top of the
    // ramp, and the pavement's far edge (when it reaches past the ramp).
    let mut st = vec![0.0, KERB_W, KERB_W + CUT_LANDING, CORNER_RIBBON];
    if width > CORNER_RIBBON + 0.01 {
        st.push(width);
    }
    let off = |o: f32| if normals.is_some() { pts.iter().zip(&nrm).map(|(p, n)| *p + *n * o).collect() } else { offset_path(&pts, out * o) };
    let lines: Vec<Vec<Vec2>> = st.iter().map(|&o| off(o)).collect();
    let far = lines.len() - 1;
    let pad_lines = [off(KERB_W + 0.03), off(KERB_W + 0.03 + TACTILE_DEPTH)];
    let top = KERB_H - 0.01;
    let kerb_y = |i: usize| KERB_H - KERB_DROP * f[i];
    let low_y = |i: usize| top - KERB_DROP * f[i];
    let v3 = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
    for i in 0..n - 1 {
        let j = i + 1;
        let d = (pts[j] - pts[i]).normalize_or_zero();
        if d == Vec2::ZERO {
            continue;
        }
        let inward = Vec2::new(-d.y, d.x) * out;
        let inward3 = Vec3::new(inward.x, 0.0, inward.y);
        let (l0, l1) = (&lines[0], &lines[1]);
        g.part(Concrete).quad_facing([v3(l0[i], kerb_y(i)), v3(l0[j], kerb_y(j)), v3(l1[j], kerb_y(j)), v3(l1[i], kerb_y(i))], Vec3::Y, c.kerb);
        g.part(Concrete).quad_facing([v3(l0[i], 0.0), v3(l0[j], 0.0), v3(l0[j], kerb_y(j)), v3(l0[i], kerb_y(i))], -inward3, c.kerb);
        if f[i] == 0.0 && f[j] == 0.0 {
            let lf = &lines[far];
            g.part(Paving).quad_facing([v3(l1[i], top), v3(l1[j], top), v3(lf[j], top), v3(lf[i], top)], Vec3::Y, c.paving);
        } else {
            let (l2, l3) = (&lines[2], &lines[3]);
            g.part(Paving).quad_facing([v3(l1[i], low_y(i)), v3(l1[j], low_y(j)), v3(l2[j], low_y(j)), v3(l2[i], low_y(i))], Vec3::Y, c.paving);
            g.part(Paving).quad_facing([v3(l2[i], low_y(i)), v3(l2[j], low_y(j)), v3(l3[j], top), v3(l3[i], top)], Vec3::Y, c.paving);
            if far > 3 {
                let lf = &lines[far];
                g.part(Paving).quad_facing([v3(l3[i], top), v3(l3[j], top), v3(lf[j], top), v3(lf[i], top)], Vec3::Y, c.paving);
            }
        }
        if pad[i].is_some() && pad[i] == pad[j] {
            let y = top - KERB_DROP + 0.006;
            let (a, b) = (&pad_lines[0], &pad_lines[1]);
            g.part(Tactile).quad_facing([v3(a[i], y), v3(a[j], y), v3(b[j], y), v3(b[i], y)], Vec3::Y, c.tactile);
        }
        if back {
            let lf = &lines[far];
            g.part(Paving).quad_facing([v3(lf[i], 0.0), v3(lf[j], 0.0), v3(lf[j], top), v3(lf[i], top)], inward3, c.paving);
        }
    }
    if caps {
        for (i, k) in [(0, 1), (n - 1, n - 2)] {
            let away = (pts[i] - pts[k]).normalize_or_zero();
            let (a, b) = (lines[0][i], lines[far][i]);
            g.part(Paving).quad_facing([v3(a, 0.0), v3(b, 0.0), v3(b, top), v3(a, kerb_y(i))], Vec3::new(away.x, 0.0, away.y), c.paving);
        }
    }
}

/// One road arriving at a node where its run ends: where its kerbs and
/// pavements begin, and which sides have them.
struct ArmEnd {
    run: usize,
    /// The run starts at the node (else it ends there).
    start: bool,
    /// The segment at the node.
    seg: SegmentId,
    /// The run's points from the node outward.
    path: Vec<Vec2>,
    hw: f32,
    /// Pavement on the counter-clockwise side of the way out (toward the
    /// next arm round), and on the clockwise side.
    pave_ccw: bool,
    pave_cw: bool,
    /// A one-way link with no pavements of its own (`Segment::ramp`): a
    /// neighbour's kerb rounds the corner onto it and stops.
    stub: bool,
    ang: f32,
}

/// The kerb round one corner of a junction, from one arm's pavement to the
/// next arm's counter-clockwise.
pub(crate) struct Corner {
    /// The kerb face, with the road on its left: from the first arm's kerb
    /// in toward the junction, round, and out along the second arm's.
    pub kerb: Vec<Vec2>,
    /// At each point of the kerb, the direction into the pavement.
    pub normals: Vec<Vec2>,
    /// Asphalt between a rounded kerb and where the kerb lines would meet.
    pub apron: Vec<Vec2>,
    /// Flat paving behind the kerb's ribbon, out to the arms' back edges.
    pub fill: Vec<Vec2>,
    /// The fill's far edge, closed with a side face.
    pub rim: Vec<Vec2>,
    /// The two arms: (run, starts here, segment).
    pub arms: [(usize, bool, SegmentId); 2],
    /// Signed turn of the kerb at the corner: negative for an ordinary
    /// corner rounded off, positive for one that wraps round the junction.
    pub turn: f32,
    /// One arm has no pavement: the kerb rounds onto it and ends there.
    pub stub: bool,
}

/// Every junction corner, and how far each run's kerb on each side stays
/// back from its end to meet it.
pub(crate) struct Corners {
    pub by_node: BTreeMap<u32, Vec<Corner>>,
    /// (run, at its start, the run's +1 side) → distance from the node
    /// along the run where that kerb begins.
    pub trims: HashMap<(usize, bool, bool), f32>,
}

/// Is a closed polygon free of self-crossings?
fn simple_polygon(poly: &[Vec2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let o = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
    let crosses = |a: Vec2, b: Vec2, c: Vec2, d: Vec2| {
        let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
        d1 * d2 < 0.0 && d3 * d4 < 0.0
    };
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if crosses(poly[i], poly[(i + 1) % n], poly[j], poly[(j + 1) % n]) {
                return false;
            }
        }
    }
    true
}

/// Metres of kerb taper per metre a town street narrows by at a joint.
const TOWN_TAPER_PER_M: f32 = 4.0;

/// The longest straight a corner's rounding takes from each kerb
/// (`nd_sim::walk::CORNER_TANGENT_MAX`, which sets the crosswalks back by it).
const CORNER_TANGENT_MAX: f32 = nd_sim::walk::CORNER_TANGENT_MAX;

/// The corner between arm `a` and the next arm `b` counter-clockwise at a
/// junction, their kerbs beginning no nearer than `base` to it. The kerb is
/// rounded with a radius of 3 to 6 m by the size of the roads, and each
/// arm's kerb is set back as far as the round needs: the junction's
/// crosswalks then meet the curve itself, as they do at a real corner.
/// Returns the corner and the two set-backs, or `None` where the arms are
/// too close in angle (or too short) to round off.
fn corner_between(a: &ArmEnd, b: &ArmEnd, base: f32) -> Option<(Corner, f32, f32)> {
    // A fork at a sharp angle needs its corner far out: the kerb lines of
    // two streets 25 degrees apart meet 20-odd metres from the node. The
    // corner is only refused where it would reach past most of either arm.
    // Half a run each: the corners at its two ends never overlap.
    let (lim_a, lim_b) = ((path_length(&a.path) * 0.5).min(base + 60.0), (path_length(&b.path) * 0.5).min(base + 60.0));
    if base > lim_a || base > lim_b {
        return None;
    }
    let narrow = a.hw.min(b.hw);
    let mut r = (0.6 * narrow).clamp(3.0, 6.0);
    for _ in 0..4 {
        let (mut ea, mut eb) = (base, base);
        for step in 0..5 {
            let (pa, ta) = point_along(&a.path, ea);
            let (pb, tb) = point_along(&b.path, eb);
            let (na, nb) = (Vec2::new(-ta.y, ta.x), Vec2::new(tb.y, -tb.x));
            let (ka, kb) = (pa + na * a.hw, pb + nb * b.hw);
            let den = ta.perp_dot(tb);
            let mut kerb: Vec<Vec2>;
            let mut normals: Vec<Vec2>;
            let mut apron = Vec::new();
            let turn;
            if den.abs() < 0.05 {
                if ta.dot(tb) > 0.0 {
                    return None; // two arms leaving the same way
                }
                // Straight on (a T's through side): the kerbs simply join.
                // Where the road narrows (a lane dropped at a joint) the
                // kerb comes in over a taper along the narrower arm, the
                // asphalt filled out to it, instead of a step across.
                let dw = a.hw - b.hw;
                if dw.abs() > 0.5 {
                    let l = (dw.abs() * TOWN_TAPER_PER_M).clamp(4.0, 14.0);
                    let need = if dw > 0.0 { (0.0, base + l - eb) } else { (base + l - ea, 0.0) };
                    if (need.0 > 0.02 || need.1 > 0.02) && step < 4 {
                        ea += need.0.max(0.0);
                        eb += need.1.max(0.0);
                        if ea > lim_a || eb > lim_b {
                            break;
                        }
                        continue;
                    }
                    // From the wider arm's kerb back to the node along its
                    // edge, across to the narrower's edge and out to where
                    // its kerb begins.
                    let node = pa - ta * ea;
                    apron = if dw > 0.0 { vec![ka, node + na * a.hw, node + nb * b.hw, kb] } else { vec![kb, node + nb * b.hw, node + na * a.hw, ka] };
                }
                kerb = vec![ka, kb];
                normals = vec![na, nb];
                turn = 0.0;
            } else {
                let w = kb - ka;
                let (sa, sb) = (w.perp_dot(tb) / den, w.perp_dot(ta) / den);
                let x = ka + ta * sa;
                let (u1, u2) = (-ta, tb);
                turn = u1.perp_dot(u2).atan2(u1.dot(u2));
                // A corner wrapping round the junction (the outside of a Y)
                // is rounded no wider than the roads, or it would cut into
                // their carriageways.
                let rr = if turn < 0.0 { r } else { r.min(narrow) };
                // The nose of a sharp fork is rounded tightly: at 25 degrees
                // a 3 m radius runs 13 m along each kerb.
                let rr = rr.min(CORNER_TANGENT_MAX / (turn.abs() * 0.5).tan().max(1e-3));
                let td = rr * (turn.abs() * 0.5).tan();
                let (need_a, need_b) = (sa + td, sb + td);
                if (need_a > 0.02 || need_b > 0.02) && step < 4 {
                    ea += need_a.max(0.0);
                    eb += need_b.max(0.0);
                    if ea > lim_a || eb > lim_b {
                        break;
                    }
                    continue;
                }
                let (ti, tj) = (x + ta * td, x + tb * td);
                kerb = vec![ka, ti];
                normals = vec![na, na];
                let centre = ti + Vec2::new(-u1.y, u1.x) * rr * turn.signum();
                let steps = (turn.abs() / 0.21).ceil().max(1.0) as usize;
                for k in 1..steps {
                    let q = centre + Vec2::from_angle(turn * k as f32 / steps as f32).rotate(ti - centre);
                    kerb.push(q);
                    // Toward the centre round an ordinary corner, away from
                    // it round one wrapping the junction.
                    normals.push((q - centre).normalize_or_zero() * turn.signum());
                }
                kerb.push(tj);
                normals.push(nb);
                if turn < 0.0 && td > 0.05 {
                    apron.push(x);
                    apron.extend(kerb[1..].iter().copied());
                }
                kerb.push(kb);
                normals.push(nb);
            }
            if ea > lim_a || eb > lim_b {
                break;
            }
            // Points closer than 2 cm are one (Ti is often the kerb's start).
            let mut k = 1;
            while k < kerb.len() {
                if kerb[k].distance(kerb[k - 1]) < 0.02 {
                    let at = if k == kerb.len() - 1 { k - 1 } else { k };
                    kerb.remove(at);
                    normals.remove(at);
                } else {
                    k += 1;
                }
            }
            if kerb.len() < 2 {
                return None;
            }
            let (oa, ob) = (pa + na * (a.hw + SIDEWALK), pb + nb * (b.hw + SIDEWALK));
            let back: Vec<Vec2> = kerb.iter().zip(&normals).map(|(p, n)| *p + *n * CORNER_RIBBON).collect();
            let mut rim = vec![ob];
            if den.abs() >= 0.05 {
                let w = ob - oa;
                let (sa, sb) = (w.perp_dot(tb) / den, w.perp_dot(ta) / den);
                // Where the arms' back edges meet. At an acute corner that
                // is a little out past where their own pavements begin (the
                // two pavements run into each other there anyway); much
                // further and the fill stops short at the arms' ends.
                if sa <= 6.0 && sb <= 6.0 {
                    rim.push(oa + ta * sa);
                }
            }
            rim.push(oa);
            // The fill's far edge: the arms' back edges to where they meet;
            // where they cross before that (a sharp corner), just the point
            // they meet at; failing both, straight across.
            let mut tries = vec![rim.clone()];
            if rim.len() == 3 {
                tries.push(vec![rim[1]]);
                tries.push(vec![rim[0], rim[2]]);
            }
            let with = |rim: &[Vec2]| back.iter().chain(rim.iter()).copied().collect::<Vec<_>>();
            let (fill, rim) = tries.into_iter().map(|r| (with(&r), r)).find(|(f, _)| simple_polygon(f)).unwrap_or_default();
            let corner = Corner { kerb, normals, apron, fill, rim, arms: [(a.run, a.start, a.seg), (b.run, b.start, b.seg)], turn, stub: false };
            return Some((corner, ea, eb));
        }
        r *= 0.6;
        if r < 1.0 {
            break;
        }
    }
    None
}

/// The corners of every junction (and of every node where two different
/// streets meet end to end) between roads with pavements: highways, ramps,
/// tunnels and footpaths have none, and a roundabout's ring has one only
/// round its outside.
fn junction_corners(roads: &RoadNetwork, net: &LaneNetwork, runs: &[Run]) -> Corners {
    let mut ends: BTreeMap<u32, Vec<ArmEnd>> = BTreeMap::new();
    let pave = |sid: SegmentId| {
        let s = roads.segment(sid);
        s.is_street() && !s.is_motorway() && !s.tunnel
    };
    for (ri, run) in runs.iter().enumerate() {
        let n = run.points.len();
        if run.kind != RoadKind::Street || n < 2 {
            continue;
        }
        for start in [true, false] {
            let (node, path, seg) = if start {
                (run.start_node, run.points.clone(), segment_between(roads, run.nodes[0], run.nodes[1]))
            } else {
                (run.end_node, run.points.iter().rev().copied().collect::<Vec<_>>(), segment_between(roads, run.nodes[n - 1], run.nodes[n - 2]))
            };
            let Some(seg) = seg else { continue };
            let sg = roads.segment(seg);
            let d = (path[1] - path[0]).normalize_or_zero();
            if d == Vec2::ZERO {
                continue;
            }
            let hw = sg.half_width();
            let (mut ccw, mut cw) = (pave(seg), pave(seg));
            if sg.ring {
                // Only the outside of the ring has a pavement; its inside
                // is the mountable apron.
                match roads.ring_of_segment(seg).and_then(|k| roads.rings.get(k)) {
                    Some(ring) => {
                        let probe = path[0] + d * 2.0 + Vec2::new(-d.y, d.x) * hw;
                        ccw = ccw && probe.distance(ring.center) > ring.radius;
                        cw = cw && !ccw;
                    }
                    None => (ccw, cw) = (false, false),
                }
            }
            let stub = sg.ramp && !sg.tunnel && !sg.ring && !sg.is_highway();
            ends.entry(node.0).or_default().push(ArmEnd { run: ri, start, seg, path, hw, pave_ccw: ccw, pave_cw: cw, stub, ang: d.y.atan2(d.x) });
        }
    }
    let mut out = Corners { by_node: BTreeMap::new(), trims: HashMap::new() };
    for (node, mut arms) in ends {
        let n = arms.len();
        if n < 2 {
            continue;
        }
        arms.sort_by(|x, y| x.ang.total_cmp(&y.ang).then(x.run.cmp(&y.run)).then(x.start.cmp(&y.start)));
        let base = net.node_radius.get(node as usize).copied().unwrap_or(4.0) + 0.6;
        for i in 0..n {
            let (a, b) = (&arms[i], &arms[(i + 1) % n]);
            // Two pavements meet round the corner; a pavement next to a
            // one-way link without any rounds onto the link and ends, so
            // the corner is not left bare either way.
            let stub = !(a.pave_ccw && b.pave_cw);
            if stub && !((a.pave_ccw && b.stub) || (a.stub && b.pave_cw)) {
                continue;
            }
            let Some((mut corner, ea, eb)) = corner_between(a, b, base) else { continue };
            corner.stub = stub;
            // The counter-clockwise side of the way out is the run's +1
            // side where the run starts here, its -1 side where it ends.
            out.trims.insert((a.run, a.start, a.start), ea);
            out.trims.insert((b.run, b.start, !b.start), eb);
            out.by_node.entry(node).or_default().push(corner);
        }
    }
    out
}

/// A pole meant to stand on the pavement by a junction, moved back behind
/// a corner's kerb (by `clear`) if the rounded corner left it in the road.
fn behind_kerb(p: Vec2, corners: Option<&Vec<Corner>>, clear: f32) -> Vec2 {
    let mut best: Option<(f32, Vec2)> = None;
    for corner in corners.into_iter().flatten() {
        for w in corner.kerb.windows(2) {
            let v = (w[1] - w[0]).normalize_or_zero();
            if v == Vec2::ZERO {
                continue;
            }
            let (_, q) = nd_sim::roads::project_on_segment(p, w[0], w[1]);
            let dist = p.distance(q);
            if dist > 4.0 || best.is_some_and(|(d, _)| d <= dist) {
                continue;
            }
            // The pavement is on the kerb's right.
            let block = Vec2::new(v.y, -v.x);
            best = Some((dist, if (p - q).dot(block) < clear { q + block * clear } else { p }));
        }
    }
    best.map(|(_, q)| q).unwrap_or(p)
}

#[cfg(test)]
mod tests {
    /// The tube's every face looks into it, so back-face culling cuts away
    /// whichever half faces the camera; seen from straight above, nothing of
    /// the vault is drawn over the carriageway, from the side the near
    /// wall goes and the far one stays.
    #[test]
    fn bore_faces_look_inwards() {
        let walk = vec![Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(80.0, 20.0)];
        let hw = 4.0;
        let mut g = Geometry::default();
        bore_shell(&mut g, &walk, hw, [1.0; 4], [0.5, 0.5, 0.5, 1.0], [2.0; 4]);
        let m = g.get(Concrete);
        assert!(m.indices.len() > 100);
        let axis = |p: Vec3| {
            // The nearest point of the walk's axis at the spring line.
            let q = Vec2::new(p.x, p.z);
            let (_, c) = walk.windows(2).map(|w| nd_sim::roads::project_on_segment(q, w[0], w[1])).fold((f32::MAX, Vec2::ZERO), |best, (_, c)| if c.distance(q) < best.0 { (c.distance(q), c) } else { best });
            Vec3::new(c.x, BORE_SPRING.min(p.y), c.y)
        };
        let (mut up, mut down) = (0, 0);
        for t in m.indices.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(m.positions[i as usize]));
            let n = (b - a).cross(c - a);
            if n.length() < 1e-6 {
                continue;
            }
            let mid = (a + b + c) / 3.0;
            if mid.y < 0.05 {
                continue; // the invert, facing up
            }
            assert!(n.dot(axis(mid) - mid) > -1e-4, "a face of the bore looks out at {mid}");
            // A camera straight above sees the faces whose front looks up.
            if n.y > 1e-3 {
                up += 1;
            } else if n.y < -1e-3 {
                down += 1;
            }
        }
        // The vault looks down into the tube: from above it is culled.
        assert!(down > 0 && up == 0, "vault faces up {up}, down {down}");
        // The profile meets the walls the portal arch is sized to.
        let p = bore_profile(hw);
        assert_eq!(p.first().unwrap().0, hw + TUNNEL_WALL);
        assert_eq!(p.last().unwrap().0, -(hw + TUNNEL_WALL));
        assert!(p.iter().map(|q| q.1).fold(0.0, f32::max) > 5.0);
    }

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
        let g = generate_street(&StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 3, heights: &[], inner: &[], layer: StreetLayer::All, closed: &[] });
        for (_, m) in g.iter() {
            assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())));
        }
        // Arrows in the highway's lanes: glow strips pointing +x somewhere
        // mid-carriageway, far from the join.
        let glow = g.get(Paint);
        assert!(glow.positions.iter().any(|p| p[0] > 100.0 && p[0] < 300.0 && p[2].abs() < 5.0 && (p[1] - MARK_Y - 0.003).abs() < 1e-3), "arrows on the highway");
        // A gore: hatching between the ramp and the carriageway near the join.
        assert!(glow.positions.iter().any(|p| p[0] < -2.0 && p[0] > -45.0 && p[2] < -5.0 && (p[1] - MARK_Y - 0.004).abs() < 1e-3), "chevrons in the gore");
        // No round junction pad at the join.
        let pad_y = ASPHALT_Y + 0.01;
        assert!(!g.get(Asphalt).positions.iter().any(|p| (p[1] - pad_y).abs() < 1e-4 && Vec2::new(p[0], p[2]).length() < 15.0), "no pad over the merge");
    }

    #[test]
    fn motorway_barriers_never_stand_on_another_motorways_lanes() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 3, 0);
        // A slip road peeling off at a shallow angle, as the demo's do.
        roads.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(-150.0, -30.0), Vec2::new(-200.0, -120.0)], 0, 1);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let lv = LandValue::new(400.0, 50.0);
        let pal = Palette::default();
        let g = generate_street(&StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 3, heights: &[], inner: &[], layer: StreetLayer::All, closed: &[] });
        let lanes = |p: Vec2| {
            roads.alive_segments().any(|(sid, s)| {
                let (a, b) = roads.segment_points(sid);
                nd_sim::roads::project_on_segment(p, a, b).1.distance(p) < s.lanes() as f32 * LANE_WIDTH * 0.5 - 0.3
            })
        };
        // Barrier tops (0.8 m up) never stand over any lane.
        // (Cheap streets move their concrete to the worn material.)
        let tops: Vec<[f32; 3]> = [Concrete, WornConcrete].iter().flat_map(|s| g.get(*s).positions.iter().copied()).filter(|p| (p[1] - 0.8).abs() < 1e-3).collect();
        assert!(tops.len() > 20, "the barriers are drawn");
        let bad: Vec<_> = tops.iter().filter(|p| lanes(Vec2::new(p[0], p[2]))).collect();
        assert!(bad.is_empty(), "{} barrier vertices on the lanes, e.g. {:?}", bad.len(), bad.first());
    }

    #[test]
    fn streets_are_finite_low_and_deterministic() {
        let (roads, net) = network();
        let lv = LandValue::new(400.0, 50.0);
        let pal = Palette::default();
        let inp = StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 5, heights: &[], inner: &[], layer: StreetLayer::All, closed: &[] };
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
        let posts = signal_posts(&roads, &net);
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
            let geo = generate_street(&StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 9, heights: &[], inner: &[], layer: StreetLayer::All, closed: &[] });
            for (s, m) in geo.iter() {
                assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite()) && p[1] < 20.0), "{s:?}");
            }
            tris.push((geo.get(Foliage).indices.len(), geo.get(Stone).indices.len(), signal_posts(&roads, &net).len()));
        }
        let (plain, ring) = (tris[0], tris[1]);
        assert!(plain.2 > 0 && ring.2 == 0, "the signals went with the junction: {plain:?} {ring:?}");
        assert_eq!(ring.1, plain.1, "no sculpture: the middle is open land");
    }
}

#[cfg(test)]
mod kerb_tests {
    use super::*;

    fn rich() -> LandValue {
        let mut lv = LandValue::new(600.0, 50.0);
        for v in &mut lv.values {
            *v = 0.9;
        }
        lv
    }

    fn draw(roads: &RoadNetwork, net: &LaneNetwork, heights: &[f32]) -> Geometry {
        let lv = rich();
        let pal = Palette::default();
        generate_street(&StreetInput { roads, net, land_value: &lv, palette: &pal, seed: 4, heights, inner: &[], layer: StreetLayer::All, closed: &[] })
    }

    /// Junctions of every shape: a crossroads of a wide and a narrow street,
    /// a T, a Y with an acute and two obtuse corners, and a long block with
    /// mid-block zebras and a footpath crossing.
    fn junctions() -> (RoadNetwork, LaneNetwork) {
        let mut roads = RoadNetwork::default();
        // Crossroads at the origin; its x street runs on through a T at
        // (300, 0).
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(480.0, 0.0)], 2, 2);
        roads.add_polyline(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(300.0, -200.0), Vec2::new(300.0, 0.0)], 1, 1);
        // A Y at (-300, 0): two arms 60 degrees apart.
        let a = 150f32.to_radians();
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(-300.0, 0.0) + Vec2::from_angle(a) * 200.0], 1, 1);
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(-300.0, 0.0) + Vec2::from_angle(-a) * 200.0], 1, 1);
        // A footpath over the crossroads' long y street.
        roads.add_polyline_kind(&[Vec2::new(-80.0, 120.0), Vec2::new(80.0, 120.0)], 1, 1, RoadKind::Path);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        (roads, net)
    }

    /// Upward faces of the street's ground-level surfaces, as triangles in
    /// the plane with a bounding box each.
    fn ground_tris(g: &Geometry) -> Vec<([Vec2; 3], Vec2, Vec2)> {
        let mut out = Vec::new();
        for s in [Asphalt, Paving, Concrete, Tactile, WornAsphalt, WornPaving, WornConcrete] {
            let m = g.get(s);
            for t in m.indices.chunks(3) {
                let p = [t[0], t[1], t[2]].map(|i| Vec3::from(m.positions[i as usize]));
                let n = (p[1] - p[0]).cross(p[2] - p[0]);
                if n.length() < 1e-9 || n.normalize().y.abs() < 0.5 || p.iter().any(|q| q.y > 0.5) {
                    continue;
                }
                let q = p.map(|v| Vec2::new(v.x, v.z));
                out.push((q, q[0].min(q[1]).min(q[2]), q[0].max(q[1]).max(q[2])));
            }
        }
        out
    }

    fn covered(tris: &[([Vec2; 3], Vec2, Vec2)], p: Vec2) -> bool {
        tris.iter().any(|(t, lo, hi)| {
            if p.x < lo.x - 1e-4 || p.y < lo.y - 1e-4 || p.x > hi.x + 1e-4 || p.y > hi.y + 1e-4 {
                return false;
            }
            let s = |a: Vec2, b: Vec2| (b - a).perp_dot(p - a);
            let (d0, d1, d2) = (s(t[0], t[1]), s(t[1], t[2]), s(t[2], t[0]));
            let e = 1e-4;
            (d0 >= -e && d1 >= -e && d2 >= -e) || (d0 <= e && d1 <= e && d2 <= e)
        })
    }

    /// A ramp carried on from a highway's start keeps its own width, and
    /// the joint narrows over a taper instead of stepping: asphalt reaches
    /// out past the ramp's edge just beside the joint, not further on.
    #[test]
    fn a_lane_drop_at_a_joint_tapers() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0), Vec2::new(400.0, 0.0)], 4, 0);
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(-150.0, 0.0), Vec2::new(0.0, 0.0)], 1, 0);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let g = draw(&roads, &net, &[]);
        let m = g.get(Asphalt);
        let tris: Vec<([Vec2; 3], Vec2, Vec2)> = m
            .indices
            .chunks(3)
            .map(|t| {
                let q = [t[0], t[1], t[2]].map(|i| Vec2::new(m.positions[i as usize][0], m.positions[i as usize][2]));
                (q, q[0].min(q[1]).min(q[2]), q[0].max(q[1]).max(q[2]))
            })
            .collect();
        let ramp_hw = LANE_WIDTH * 0.5 + SHOULDER;
        let highway_hw = 2.0 * LANE_WIDTH + SHOULDER;
        let beside = (ramp_hw + highway_hw) * 0.5;
        assert!(covered(&tris, Vec2::new(-2.0, beside)) && covered(&tris, Vec2::new(-2.0, -beside)), "the joint widens out to the highway");
        assert!(!covered(&tris, Vec2::new(-100.0, beside)), "the ramp further on keeps its width");
        // The highway itself is drawn its full width right up to the joint.
        assert!(covered(&tris, Vec2::new(2.0, highway_hw - 0.5)));
    }

    /// Round every junction, everything within the arms' pavements out to
    /// 25 m from the centre is road, kerb or paving: the terrain used to
    /// show in each corner between one pavement's end, the pad and the
    /// next pavement.
    #[test]
    fn junction_corners_leave_no_bare_ground() {
        let (roads, net) = junctions();
        let g = draw(&roads, &net, &[]);
        let tris = ground_tris(&g);
        let mut checked = 0;
        for (i, node) in roads.nodes.iter().enumerate() {
            let arms: Vec<(Vec2, f32)> = node
                .segments
                .iter()
                .filter(|&&s| roads.segment(s).alive && roads.segment(s).is_street())
                .map(|&s| {
                    let (a, b) = roads.segment_points(s);
                    let seg = roads.segment(s);
                    ((if seg.a.0 as usize == i { b - a } else { a - b }).normalize(), seg.half_width())
                })
                .collect();
            if arms.len() < 3 {
                continue;
            }
            let mut bare = Vec::new();
            for gx in -60..=60 {
                for gy in -60..=60 {
                    let off = Vec2::new(gx as f32, gy as f32) * 0.4;
                    let p = node.pos + off;
                    let inside = arms.iter().any(|&(d, hw)| {
                        let s = off.dot(d);
                        s >= 0.0 && s <= 22.0 && off.perp_dot(d).abs() <= hw + SIDEWALK - 0.05
                    });
                    if !inside {
                        continue;
                    }
                    checked += 1;
                    if !covered(&tris, p) {
                        bare.push(p);
                    }
                }
            }
            assert!(bare.is_empty(), "junction at {:?}: {} bare points, e.g. {:?}", node.pos, bare.len(), &bare[..bare.len().min(6)]);
        }
        assert!(checked > 3000, "{checked} points checked");
        // The corners are rounded: at the crossroads no kerb stands at the
        // sharp corner where the kerb lines meet, and the asphalt reaches it.
        let wide = 2.0 * LANE_WIDTH + SHOULDER;
        let narrow = LANE_WIDTH + SHOULDER;
        let tip = Vec2::new(narrow, wide);
        let kerb_near_tip = g.get(Concrete).positions.iter().any(|p| Vec2::new(p[0], p[2]).distance(tip) < 0.6);
        assert!(!kerb_near_tip, "the crossroads' corner is rounded off");
        assert!(g.get(Asphalt).positions.iter().any(|p| Vec2::new(p[0], p[2]).distance(tip) < 1e-3), "asphalt out to the old corner");
    }

    /// The crosswalk ends in `g` that meet no kerb; panics where a kerb is
    /// met but not dropped, stands full height across the crosswalk, or has
    /// no tactile pad behind it.
    fn crosswalk_ends_without_kerb(roads: &RoadNetwork, g: &Geometry) -> Vec<(Vec2, f32)> {
        let cuts = crossing_cuts(roads);
        let all: Vec<(u32, Cut)> = cuts.iter().flat_map(|(s, v)| v.iter().map(move |c| (*s, *c))).collect();
        let kerb = g.get(Concrete);
        let pads = g.get(Tactile);
        let mut no_kerb = Vec::new();
        for (sid, c) in &all {
            let hw = roads.segment(SegmentId(*sid)).half_width();
            for side in [-1.0f32, 1.0] {
                // (u along the street, v out from its centre line on this side)
                let local = |p: &[f32; 3]| {
                    let q = Vec2::new(p[0], p[2]) - c.mid;
                    (q.dot(c.along), -q.perp_dot(c.along) * side)
                };
                if !kerb.positions.iter().map(local).any(|(u, v)| u.abs() <= c.half + 0.01 && v > hw - 0.1 && v < hw + 2.0) {
                    no_kerb.push((c.mid, side));
                    continue;
                }
                let low = kerb.positions.iter().filter(|p| (p[1] - KERB_LOW).abs() < 0.004).map(local).filter(|&(u, v)| u.abs() <= c.half + 0.01 && v > hw - 0.1 && v < hw + 2.0).count();
                assert!(low >= 2, "crosswalk at {:?} (side {side}): no dropped kerb", c.mid);
                let high = kerb.positions.iter().filter(|p| (p[1] - KERB_H).abs() < 0.004).map(local).find(|&(u, v)| u.abs() < c.half - 0.05 && v > hw - 0.1 && v < hw + 0.4);
                assert!(high.is_none(), "crosswalk at {:?} (side {side}): a full-height kerb across it at {high:?}", c.mid);
                let pad = pads.positions.iter().map(local).filter(|&(u, v)| u.abs() <= c.half + 0.05 && v > hw && v < hw + 3.0).count();
                assert!(pad >= 4, "crosswalk at {:?} (side {side}): no tactile pad", c.mid);
            }
        }
        no_kerb
    }

    /// Every crosswalk end meets a dropped kerb with a tactile pad behind
    /// it, and nowhere across the crosswalk does the kerb stand full height.
    #[test]
    fn every_crosswalk_end_gets_a_kerb_drop_and_a_tactile_pad() {
        let (roads, net) = junctions();
        let g = draw(&roads, &net, &[]);
        let cuts = crossing_cuts(&roads);
        let all: Vec<(u32, Cut)> = cuts.iter().flat_map(|(s, v)| v.iter().map(move |c| (*s, *c))).collect();
        let kinds = (
            all.iter().filter(|(_, c)| c.half == nd_sim::walk::CROSSWALK_HALF).count(),
            all.iter().filter(|(_, c)| c.half == 1.3).count(),
        );
        assert!(kinds.0 >= 10 && kinds.1 >= 2, "junction, mid-block and path crossings: {kinds:?}");
        assert!(!nd_sim::walk::midblock_crossings(&roads).is_empty(), "the long block has a mid-block zebra");
        let no_kerb = crosswalk_ends_without_kerb(&roads, &g);
        assert!(no_kerb.is_empty(), "crosswalk ends that meet no kerb: {no_kerb:?}");
        // The pads lie low, on the landing behind the dropped kerb.
        assert!(g.get(Tactile).positions.iter().all(|p| p[1] < KERB_LOW + 0.01));
    }

    /// [review] A Y of three narrow streets, two of them 60 degrees apart:
    /// the crosswalks either side of the sharp corner used to end on the
    /// asphalt where the two arms run together, short of where their kerb
    /// lines even meet, so the corner got no curb cut. They are set out to
    /// the rounded kerb now (`nd_sim::walk::crosswalk_setback`).
    #[test]
    fn a_sharp_corner_gets_its_curb_cuts() {
        for deg in [60f32, 50.0, 70.0] {
            let mut roads = RoadNetwork::default();
            let a = (180.0 - deg * 0.5).to_radians();
            roads.add_polyline(&[Vec2::ZERO, Vec2::new(120.0, 0.0)], 1, 1);
            roads.add_polyline(&[Vec2::ZERO, Vec2::from_angle(a) * 120.0], 1, 1);
            roads.add_polyline(&[Vec2::ZERO, Vec2::from_angle(-a) * 120.0], 1, 1);
            let mut net = LaneNetwork::default();
            net.rebuild(&roads);
            let g = draw(&roads, &net, &[]);
            assert_eq!(crossing_cuts(&roads).values().map(Vec::len).sum::<usize>(), 3, "three crosswalks");
            let no_kerb = crosswalk_ends_without_kerb(&roads, &g);
            assert!(no_kerb.is_empty(), "{deg} degrees: crosswalk ends that meet no kerb: {no_kerb:?}");
        }
    }

    /// Curb cuts and corners are cheap: a pad is a couple of triangles (the
    /// dots are in the texture), a curb cut a few dozen, a corner a couple
    /// of hundred, so a city with thousands of crossings stays in budget.
    #[test]
    fn kerbs_stay_within_the_triangle_budget() {
        let cols = KerbColours { kerb: [1.0; 4], paving: [1.0; 4], tactile: [1.0; 4] };
        let face = [Vec2::new(0.0, 5.0), Vec2::new(60.0, 5.0)];
        let mut plain = Geometry::default();
        kerb_ribbon(&mut plain, &face, None, -1.0, SIDEWALK, true, true, &[], &cols);
        let cut = Cut { mid: Vec2::new(30.0, 0.0), along: Vec2::X, half: nd_sim::walk::CROSSWALK_HALF, reach: 5.0 + SIDEWALK + 0.5 };
        let mut dropped = Geometry::default();
        kerb_ribbon(&mut dropped, &face, None, -1.0, SIDEWALK, true, true, &[cut], &cols);
        let extra = dropped.triangle_count() - plain.triangle_count();
        assert!(extra <= 60, "a curb cut costs {extra} triangles");
        assert!(dropped.get(Tactile).indices.len() / 3 <= 4, "a pad is a quad or two");
        assert!(plain.triangle_count() <= 12, "a plain kerb is a few quads: {}", plain.triangle_count());

        let (roads, net) = junctions();
        let runs = build_runs(&roads);
        let corners = junction_corners(&roads, &net, &runs);
        let n: usize = corners.by_node.values().map(|v| v.len()).sum();
        assert!(n >= 4 + 3 + 3, "{n} corners");
        let g = draw(&roads, &net, &[]);
        let crossings: usize = crossing_cuts(&roads).values().map(|v| v.len()).sum();
        let tactile = g.get(Tactile).indices.len() / 3;
        assert!(tactile <= crossings * 2 * 8, "{tactile} pad triangles for {crossings} crossings");
        for list in corners.by_node.values() {
            for c in list {
                let mut one = Geometry::default();
                kerb_ribbon(&mut one, &c.kerb, Some(&c.normals), -1.0, CORNER_RIBBON, false, false, &[], &cols);
                one.part(Paving).ground_polygon(&c.fill, KERB_H, [1.0; 4]);
                one.part(Asphalt).ground_polygon(&c.apron, ASPHALT_Y, [1.0; 4]);
                assert!(one.triangle_count() <= 220, "a corner costs {} triangles", one.triangle_count());
            }
        }
    }

    /// On a sloping street the corners stand at the road's height, not the
    /// junction's: the kerb where a corner meets a run is at the run's
    /// height, so there is no step or gap between them.
    #[test]
    fn corners_follow_the_slope_of_their_streets() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, 100.0)], 1, 1);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        // Rising 6 m over 100 m to the east, flat elsewhere.
        let heights: Vec<f32> = roads.nodes.iter().map(|n| if n.pos.x > 50.0 { 6.0 } else { 0.0 }).collect();
        let g = draw(&roads, &net, &heights);
        for (s, m) in g.iter() {
            assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())), "{s:?}");
        }
        // Kerb tops round the east corners stand at the street's height
        // there (6 m over 100 m), within a few centimetres.
        let kerb = g.get(Concrete);
        let mut seen = 0;
        for p in kerb.positions.iter().filter(|p| p[0] > 6.0 && p[0] < 14.0 && (p[2].abs() - 5.0).abs() < 0.4) {
            let road = p[0] / 100.0 * 6.0;
            assert!(p[1] >= road - 0.05 && p[1] <= road + KERB_H + 0.05, "kerb at x {} stands at {} (road {road})", p[0], p[1]);
            seen += 1;
        }
        assert!(seen > 4);
    }

    /// A roundabout's ring has a pavement round its outside that the arms'
    /// pavements run into, and none on its inside.
    #[test]
    fn a_roundabout_ring_is_paved_round_the_outside() {
        use nd_sim::plan::{Gesture, GestureId, Plan};
        let mut plan = Plan::default();
        plan.gestures.insert(GestureId(1), Gesture::Road { points: vec![Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        plan.gestures.insert(GestureId(2), Gesture::Road { points: vec![Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        plan.gestures.insert(GestureId(3), Gesture::Roundabout { center: Vec2::ZERO, radius: 30.0, lanes: 1 });
        let (built, rings) = plan.built_roads();
        let mut roads = RoadNetwork::default();
        roads.rings = rings;
        for (_, pts, f, b, k) in built {
            roads.add_polyline_kind(&pts, f, b, k);
        }
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let ring = roads.rings[0];
        let g = draw(&roads, &net, &[]);
        let tris = ground_tris(&g);
        let out = ring.radius + ring.half_width() + 1.5;
        let bare: Vec<Vec2> = (0..72).map(|k| ring.center + Vec2::from_angle(k as f32 * 5f32.to_radians()) * out).filter(|&p| !covered(&tris, p)).collect();
        assert!(bare.is_empty(), "bare ground round the ring at {bare:?}");
        let kerb_inside = g.get(Concrete).positions.iter().any(|p| {
            let d = Vec2::new(p[0], p[2]).distance(ring.center);
            d < ring.radius - ring.half_width() + 0.5 && d > ring.radius - ring.half_width() - 3.0
        });
        assert!(!kerb_inside, "no pavement inside the ring");
    }

    /// Prints the street triangles of a save, for comparing before and
    /// after a change: `ND_STREET_SAVE=path cargo test --release -p
    /// nd_procgen street_triangles_of_a_save -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn street_triangles_of_a_save() {
        let Ok(path) = std::env::var("ND_STREET_SAVE") else { return };
        let w = nd_sim::world::World::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
        let heights: &[f32] = if w.relief { &w.road_z } else { &[] };
        let inner: &[Vec<(f32, f32)>] = if w.relief { &w.road_inner } else { &[] };
        let pal = Palette::default();
        let t = std::time::Instant::now();
        let g = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &pal, seed: w.config.seed, heights, inner, layer: StreetLayer::Surface, closed: &[] });
        let ms = t.elapsed().as_millis();
        let cuts: usize = crossing_cuts(&w.roads).values().map(|v| v.len()).sum();
        let corners: usize = junction_corners(&w.roads, &w.traffic.net, &build_runs(&w.roads)).by_node.values().map(|v| v.len()).sum();
        println!("street triangles {} (tactile {}), {cuts} crossings, {corners} corners, {ms} ms", g.triangle_count(), g.get(Tactile).indices.len() / 3);
        if let Ok(n) = std::env::var("ND_STREET_NODE") {
            let nid = NodeId(n.parse().unwrap());
            let runs = build_runs(&w.roads);
            for &s in &w.roads.node(nid).segments {
                let sg = w.roads.segment(s);
                println!("seg {} alive {} kind {:?} lanes {}+{} ramp {} ring {} tunnel {} hw {}", s.0, sg.alive, sg.kind, sg.lanes_fwd, sg.lanes_bwd, sg.ramp, sg.ring, sg.tunnel, sg.half_width());
            }
            let c = junction_corners(&w.roads, &w.traffic.net, &runs);
            for (ri, r) in runs.iter().enumerate().filter(|(_, r)| r.start_node == nid || r.end_node == nid) {
                println!("run {ri} len {} start {} trims {:?} {:?} {:?} {:?}", path_length(&r.points), r.start_node == nid, c.trims.get(&(ri, true, true)), c.trims.get(&(ri, true, false)), c.trims.get(&(ri, false, true)), c.trims.get(&(ri, false, false)));
            }
            println!("corners {}", c.by_node.get(&nid.0).map_or(0, |v| v.len()));
        }
        if std::env::var("ND_STREET_PLACES").is_ok() {
            // Where to look: signalised junctions, Ts, mid-block zebras and
            // crossings on a slope.
            for i in w.traffic.net.intersections.iter().filter(|i| i.signalized).take(3) {
                println!("signalised {:?}", w.roads.node(i.node).pos);
            }
            for (k, n) in w.roads.nodes.iter().enumerate().filter(|(k, _)| w.roads.street_degree(NodeId(*k as u32)) == 3).take(3) {
                println!("T {:?} z {}", n.pos, heights.get(k).copied().unwrap_or(0.0));
            }
            for (s, t) in nd_sim::walk::midblock_crossings(&w.roads).into_iter().take(3) {
                let (a, b) = w.roads.segment_points(s);
                let sg = w.roads.segment(s);
                let dz = (heights.get(sg.a.0 as usize).copied().unwrap_or(0.0) - heights.get(sg.b.0 as usize).copied().unwrap_or(0.0)).abs() / a.distance(b);
                println!("zebra {:?} slope {dz}", a.lerp(b, t));
            }
            let mut steep: Vec<(f32, Vec2)> = crossing_cuts(&w.roads)
                .iter()
                .map(|(s, v)| {
                    let sg = w.roads.segment(SegmentId(*s));
                    let (a, b) = w.roads.segment_points(SegmentId(*s));
                    ((heights.get(sg.a.0 as usize).copied().unwrap_or(0.0) - heights.get(sg.b.0 as usize).copied().unwrap_or(0.0)).abs() / a.distance(b).max(1.0), v[0].mid)
                })
                .collect();
            steep.sort_by(|a, b| b.0.total_cmp(&a.0));
            for (dz, p) in steep.iter().take(3) {
                println!("sloped crossing {p:?} slope {dz}");
            }
        }
    }
}

/// Lane arrows painted on each lane arriving at a junction, a few metres
/// before the stop line: one arrow per lane with a head for every movement
/// it allows (`Lane::arrows`), so a left-only lane reads differently from a
/// straight-and-right one. Separate from the street mesh because arrows
/// follow turning demand and the player's choices without the roads
/// changing; rebuild when `LaneNetwork::arrows_version` moves.
///
/// Drawn at road-marking scale (about 5 m long, 0.4 m shafts, filled
/// heads 1.2 m across) in bright paint: the first version used 0.22 m
/// strokes on the dimmed glow material and could not be read from the
/// top-down camera or in daylight. A lane of 9 m or more gets one; long
/// lanes get a second 25 m further back, as real approaches do, so the
/// arrows are seen before the queue covers the first.
pub fn lane_arrows(roads: &RoadNetwork, net: &LaneNetwork, heights: &[f32], inner: &[Vec<(f32, f32)>], pal: &Palette) -> Geometry {
    let mut g = Geometry::default();
    let c = rgba(mul(pal.lane_marker, 1.25));
    let y = MARK_Y + 0.003;
    let cuts = crossing_cuts(roads);
    let radius = |n: NodeId| net.node_radius.get(n.0 as usize).copied().unwrap_or(4.0);
    // [interchange] Where an arm's lanes end at a node (further out at a
    // sharp fork: `LaneNetwork::stop_back`).
    let lane_end = |n: NodeId, s: SegmentId| radius(n) + net.stop_back.get(&(n, s)).copied().unwrap_or(0.0);
    for ap in &net.approaches {
        // How far the painted stop line stands before the lane's end (the
        // lanes end at the junction's radius; the line is drawn beyond the
        // crosswalk, `junction::mouths`), and how much of the lane's start
        // the junction behind it takes.
        let ahead = if pad_node(roads, ap.node) {
            mouths(roads, net, ap.node).iter().find(|m| m.seg == ap.segment).map_or(0.0, |m| (m.marks - lane_end(ap.node, ap.segment)).max(0.0))
        } else {
            0.0
        };
        let seg = roads.segment(ap.segment);
        let from = if seg.a == ap.node { seg.b } else { seg.a };
        let behind = if pad_node(roads, from) {
            mouths(roads, net, from).iter().find(|m| m.seg == ap.segment).map_or(0.0, |m| (m.marks - lane_end(from, ap.segment)).max(0.0))
        } else {
            0.0
        };
        let seg_cuts = cuts.get(&ap.segment.0).map(|v| v.as_slice()).unwrap_or(&[]);
        let (sa, sb) = roads.segment_points(ap.segment);
        for (&l, &a) in ap.lanes.iter().zip(&ap.arrows) {
            let lane = &net.lanes[l as usize];
            if lane.length < 9.0 || a == 0 {
                continue;
            }
            // Clear of every crosswalk on the way: an arrow on a short
            // block used to land on the zebra of the junction behind.
            let on_zebra = |p: Vec2| seg_cuts.iter().any(|k| k.near(p) && k.u(p).abs() < k.half + 0.8);
            // The front 4 m before the stop line (8 m before the lane's end
            // at an ordinary junction), nearer on a short lane; the arrow is
            // 5.4 m from tail to tip.
            let mut back = (ahead + 4.0).max(8.0).min(lane.length - 6.0).max(0.5);
            let mut n = 0;
            while back + 5.4 <= lane.length - behind && n < 2 {
                let (tip_at, d) = lane.point_at(lane.length - back);
                let (tail, _) = lane.point_at(lane.length - back - 5.4);
                if on_zebra(tip_at) || on_zebra(tail) || on_zebra((tip_at + tail) * 0.5) {
                    back += 2.0;
                    continue;
                }
                let from_v = g.get(Paint).positions.len();
                lane_arrow(g.part(Paint), tip_at, d, a, y, c);
                if !heights.is_empty() {
                    // At the height of its own road (the renderer used to
                    // lift it onto whichever road was nearest each vertex,
                    // a deck overhead included).
                    let len = sa.distance(sb).max(1e-3);
                    for p in g.part(Paint).positions[from_v..].iter_mut() {
                        let t = (Vec2::new(p[0], p[2]) - sa).dot(sb - sa) / (len * len);
                        p[1] += nd_sim::landscape::way_height(roads, heights, inner, ap.segment, t);
                    }
                }
                back += 25.0;
                n += 1;
            }
        }
    }
    g
}

/// One painted lane arrow whose foremost point is near `front`, pointing
/// along `d`, with heads for the movements in `arrows` (LEFT, STRAIGHT,
/// RIGHT bits). Right-hand traffic: left is the counter-clockwise side
/// seen from above, `(-d.y, d.x)` negated in sim coordinates (y south).
pub fn lane_arrow(m: &mut crate::mesh::MeshData, front: Vec2, d: Vec2, arrows: u8, y: f32, c: [f32; 4]) {
    use nd_sim::traffic::{LEFT, RIGHT, STRAIGHT};
    let right = Vec2::new(-d.y, d.x);
    let shaft = 0.4;
    let straight = arrows & STRAIGHT != 0;
    let head = |m: &mut crate::mesh::MeshData, base: Vec2, dir: Vec2, len: f32, half: f32| {
        let side = Vec2::new(-dir.y, dir.x);
        m.ground_polygon(&[base + side * half, base + dir * len, base - side * half], y, c);
    };
    // The shaft: up to the straight head, or ending where the turns leave.
    let start = front - d * 5.4;
    let end = if straight { front - d * 1.15 } else { front - d * 2.3 };
    m.ground_strip(&[start, end], shaft, y, c);
    if straight {
        head(m, front - d * 1.2, d, 1.2, 0.6);
    }
    for (bit, sgn) in [(LEFT, -1.0f32), (RIGHT, 1.0)] {
        if arrows & bit == 0 {
            continue;
        }
        // A branch off the shaft well behind the straight head, sweeping out
        // to the side and ending in a head that points across the lane
        // (and a little ahead): a turn, apart from the straight arrow.
        let side = right * sgn;
        let j = if straight { front - d * 3.6 } else { front - d * 2.5 };
        let elbow = j + d * 0.55 + side * 0.4;
        m.ground_strip(&[j - d * 0.1, elbow], shaft, y, c);
        let dir = (side + d * 0.2).normalize_or_zero();
        head(m, elbow - dir * 0.05, dir, 1.05, 0.5);
    }
}

#[cfg(test)]
mod arrow_tests {
    use super::*;
    use nd_sim::traffic::{LEFT, RIGHT, STRAIGHT};

    /// A plain crossroads of two-lane-each-way streets: every arriving lane
    /// gets a painted arrow, in the paint surface (not the dimmed glow), and
    /// the arrows are big enough to read from above (wider than a lane line).
    #[test]
    fn junction_lanes_get_readable_arrows() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 2, 2);
        roads.add_polyline(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], 2, 2);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let g = lane_arrows(&roads, &net, &[], &[], &Palette::default());
        assert!(g.get(Glow).is_empty(), "arrows are paint, not glow");
        let paint = g.get(Paint);
        assert!(!paint.is_empty());
        let approach_lanes: usize = net.approaches.iter().map(|a| a.arrows.iter().filter(|&&x| x != 0).count()).sum();
        assert!(approach_lanes >= 8, "{approach_lanes}");
        // Bright: markedly brighter than the plain lane marker.
        let pal = Palette::default();
        assert!(paint.colors.iter().all(|c| c[0] > pal.lane_marker[0]));
    }

    /// The heads follow the movements: a left-only arrow reaches out to the
    /// driver's left, a right-only one to the right, a straight one only ahead.
    #[test]
    fn arrow_heads_follow_the_allowed_movements() {
        let d = Vec2::X;
        let right = Vec2::new(-d.y, d.x);
        let extent = |a: u8| {
            let mut m = crate::mesh::MeshData::default();
            lane_arrow(&mut m, Vec2::ZERO, d, a, 0.0, [1.0; 4]);
            let side: Vec<f32> = m.positions.iter().map(|p| Vec2::new(p[0], p[2]).dot(right)).collect();
            (side.iter().cloned().fold(f32::MAX, f32::min), side.iter().cloned().fold(f32::MIN, f32::max))
        };
        let (l0, l1) = extent(LEFT);
        let (r0, r1) = extent(RIGHT);
        let (s0, s1) = extent(STRAIGHT);
        assert!(l0 < -0.9 && l1 < 0.7, "left reaches left: {l0} {l1}");
        assert!(r1 > 0.9 && r0 > -0.7, "right reaches right: {r0} {r1}");
        assert!(s0 > -0.7 && s1 < 0.7, "straight stays in line: {s0} {s1}");
        let (b0, b1) = extent(LEFT | STRAIGHT | RIGHT);
        assert!(b0 < -0.9 && b1 > 0.9);
    }
}

#[cfg(test)]
mod sweep_visual_tests {
    use super::*;

    /// [sweep-visual] `clear_along` follows an arm past its first piece to
    /// where it stops overlapping a neighbour at a sharp angle, and leaves
    /// an arm that never overlaps at its start.
    #[test]
    fn clear_along_finds_where_a_sharp_arm_parts_from_its_neighbour() {
        // The neighbour: a street along +x, half width 5.
        let street = vec![(vec![Vec2::ZERO, Vec2::new(200.0, 0.0)], 5.0)];
        // A ramp leaving at 15 degrees in short pieces, half width 3.
        let d = Vec2::from_angle(15f32.to_radians());
        let ramp: Vec<Vec2> = (0..10).map(|k| d * (k as f32 * 6.0)).collect();
        let s = clear_along(&ramp, 4.0, 50.0, &[-3.0, 0.0, 3.0], &street);
        // Its near edge (3 m in) clears the street's 5 m once sin(15)*s - 3 cos(15) >= 5.
        let want = (5.0 + 3.0 * 15f32.to_radians().cos()) / 15f32.to_radians().sin();
        assert!((s - want).abs() < 0.6, "{s} vs {want}");
        assert!(s > 6.0, "past the first piece");
        // Square to it: clear at once.
        let square: Vec<Vec2> = (0..10).map(|k| Vec2::new(0.0, 6.0 + k as f32 * 6.0)).collect();
        assert_eq!(clear_along(&square, 4.0, 50.0, &[-3.0, 0.0, 3.0], &street), 4.0);
    }
}
