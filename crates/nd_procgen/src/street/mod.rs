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

mod arrows;
mod junction;
mod kerb;
mod rail_path;
mod signals;
mod tunnel;
#[cfg(test)]
mod arrow_tests;
#[cfg(test)]
mod kerb_tests;
#[cfg(test)]
mod sweep_visual_tests;
#[cfg(test)]
mod tests;

pub use arrows::*;
pub(crate) use kerb::*;
use rail_path::*;
pub use signals::*;
pub use tunnel::*;
use crate::depth::{ASPHALT_Y, KERB_H, MARK_Y};
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
