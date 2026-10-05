//! Road network: a node/segment graph with snapping, intersection splitting
//! and A* pathfinding.

use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SegmentId(pub u32);

/// Nodes closer than this are merged when roads are drawn.
pub const SNAP_RADIUS: f32 = 5.0;
pub const LANE_WIDTH: f32 = 3.5;
/// Extra asphalt beyond the lanes (sidewalk / shoulder).
pub const SHOULDER: f32 = 1.5;
/// How far a road end may be dropped from an existing road and still join it.
pub const END_SNAP_RADIUS: f32 = 9.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub pos: Vec2,
    pub segments: Vec<SegmentId>,
}

/// What a road carries. Streets have car lanes and pavements; paths are for
/// people on foot and on bikes only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoadKind {
    #[default]
    Street,
    Path,
}

/// Half the width of a path, in metres.
pub const PATH_HALF_WIDTH: f32 = 1.6;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Segment {
    pub a: NodeId,
    pub b: NodeId,
    /// Lanes running a -> b.
    pub lanes_fwd: u8,
    /// Lanes running b -> a.
    pub lanes_bwd: u8,
    pub alive: bool,
    #[serde(default)]
    pub kind: RoadKind,
}

impl Segment {
    pub fn lanes(&self) -> u8 {
        self.lanes_fwd + self.lanes_bwd
    }

    pub fn is_street(&self) -> bool {
        self.kind == RoadKind::Street
    }

    pub fn half_width(&self) -> f32 {
        match self.kind {
            RoadKind::Street => self.lanes() as f32 * LANE_WIDTH * 0.5 + SHOULDER,
            RoadKind::Path => PATH_HALF_WIDTH,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RoadNetwork {
    pub nodes: Vec<Node>,
    pub segments: Vec<Segment>,
    /// Incremented on every change so renderers can cheaply detect updates.
    pub version: u64,
}

impl RoadNetwork {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    pub fn segment(&self, id: SegmentId) -> &Segment {
        &self.segments[id.0 as usize]
    }

    pub fn segment_points(&self, id: SegmentId) -> (Vec2, Vec2) {
        let s = self.segment(id);
        (self.node(s.a).pos, self.node(s.b).pos)
    }

    pub fn alive_segments(&self) -> impl Iterator<Item = (SegmentId, &Segment)> {
        self.segments
            .iter()
            .enumerate()
            .filter(|(_, s)| s.alive)
            .map(|(i, s)| (SegmentId(i as u32), s))
    }

    pub fn is_empty(&self) -> bool {
        !self.segments.iter().any(|s| s.alive)
    }

    pub fn node_near(&self, p: Vec2, radius: f32) -> Option<NodeId> {
        let mut best: Option<(f32, NodeId)> = None;
        for (i, n) in self.nodes.iter().enumerate() {
            let d = n.pos.distance(p);
            if d <= radius && best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, NodeId(i as u32)));
            }
        }
        best.map(|(_, id)| id)
    }

    fn get_or_add_node(&mut self, p: Vec2) -> NodeId {
        if let Some(id) = self.node_near(p, SNAP_RADIUS) {
            return id;
        }
        self.nodes.push(Node { pos: p, segments: Vec::new() });
        NodeId((self.nodes.len() - 1) as u32)
    }

    /// Node for a road end: an existing node nearby, else a new node on the
    /// nearest road within `radius` (splitting it into a junction), else a
    /// fresh node at `p`.
    fn end_node(&mut self, p: Vec2, radius: f32) -> NodeId {
        if let Some(id) = self.node_near(p, SNAP_RADIUS) {
            return id;
        }
        if let Some((sid, _t, q)) = self.nearest_on_network(p) {
            if q.distance(p) <= radius {
                if let Some(id) = self.node_near(q, SNAP_RADIUS) {
                    return id;
                }
                self.nodes.push(Node { pos: q, segments: Vec::new() });
                let id = NodeId((self.nodes.len() - 1) as u32);
                self.split_segment(sid, id);
                return id;
            }
        }
        self.nodes.push(Node { pos: p, segments: Vec::new() });
        NodeId((self.nodes.len() - 1) as u32)
    }

    /// Where a road end dropped at `p` would land: a node, a point on a road,
    /// or `p` itself. Used by the tools to show the snap before committing.
    pub fn snap_point(&self, p: Vec2, radius: f32) -> (Vec2, bool) {
        if let Some(id) = self.node_near(p, radius) {
            return (self.node(id).pos, true);
        }
        if let Some((sid, _, q)) = self.nearest_on_network(p) {
            if q.distance(p) <= self.segment(sid).half_width() + radius {
                return (q, true);
            }
        }
        (p, false)
    }

    fn find_link(&self, a: NodeId, b: NodeId) -> Option<SegmentId> {
        self.node(a).segments.iter().copied().find(|&sid| {
            let s = self.segment(sid);
            s.alive && ((s.a == a && s.b == b) || (s.a == b && s.b == a))
        })
    }

    /// Number of alive streets at a node (paths do not count).
    pub fn street_degree(&self, n: NodeId) -> usize {
        self.node(n).segments.iter().filter(|&&s| self.segment(s).alive && self.segment(s).is_street()).count()
    }

    fn link(&mut self, a: NodeId, b: NodeId, lanes_fwd: u8, lanes_bwd: u8, kind: RoadKind) -> SegmentId {
        if let Some(existing) = self.find_link(a, b) {
            return existing;
        }
        self.segments.push(Segment { a, b, lanes_fwd, lanes_bwd, alive: true, kind });
        let id = SegmentId((self.segments.len() - 1) as u32);
        self.nodes[a.0 as usize].segments.push(id);
        self.nodes[b.0 as usize].segments.push(id);
        id
    }

    fn split_segment(&mut self, sid: SegmentId, at: NodeId) {
        let s = self.segments[sid.0 as usize].clone();
        self.segments[sid.0 as usize].alive = false;
        for n in [s.a, s.b] {
            self.nodes[n.0 as usize].segments.retain(|&x| x != sid);
        }
        self.link(s.a, at, s.lanes_fwd, s.lanes_bwd, s.kind);
        self.link(at, s.b, s.lanes_fwd, s.lanes_bwd, s.kind);
    }

    /// Add a road along `points`. Crossing existing segments splits them so
    /// intersections become real nodes. Returns the created segments.
    pub fn add_polyline(&mut self, points: &[Vec2], lanes_fwd: u8, lanes_bwd: u8) -> Vec<SegmentId> {
        self.add_polyline_kind(points, lanes_fwd, lanes_bwd, RoadKind::Street)
    }

    /// Add a road or path along `points`.
    pub fn add_polyline_kind(&mut self, points: &[Vec2], lanes_fwd: u8, lanes_bwd: u8, kind: RoadKind) -> Vec<SegmentId> {
        let mut out = Vec::new();
        let last = points.len().saturating_sub(1);
        for (i, w) in points.windows(2).enumerate() {
            if w[0].distance(w[1]) < 0.5 {
                continue;
            }
            // Road ends snap onto whatever road they were dropped near so a
            // T-junction forms; interior points only split roads they cross.
            let a = if i == 0 { self.end_node(w[0], END_SNAP_RADIUS) } else { self.get_or_add_node(w[0]) };
            let b = if i + 1 == last { self.end_node(w[1], END_SNAP_RADIUS) } else { self.get_or_add_node(w[1]) };
            out.extend(self.add_segment_between(a, b, lanes_fwd, lanes_bwd, kind));
        }
        if !out.is_empty() {
            self.version += 1;
        }
        out
    }

    fn add_segment_between(&mut self, a: NodeId, b: NodeId, lanes_fwd: u8, lanes_bwd: u8, kind: RoadKind) -> Vec<SegmentId> {
        if a == b {
            return Vec::new();
        }
        let pa = self.node(a).pos;
        let pb = self.node(b).pos;

        let mut cuts: Vec<(f32, NodeId)> = Vec::new();
        let count = self.segments.len();
        for i in 0..count {
            let s = self.segments[i].clone();
            if !s.alive || s.a == a || s.a == b || s.b == a || s.b == b {
                continue;
            }
            let (p0, p1) = (self.node(s.a).pos, self.node(s.b).pos);
            if let Some((t, _u)) = segment_intersection(pa, pb, p0, p1) {
                let x = pa.lerp(pb, t);
                let n = self.get_or_add_node(x);
                if n != s.a && n != s.b {
                    self.split_segment(SegmentId(i as u32), n);
                }
                cuts.push((t, n));
            }
        }
        cuts.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(Ordering::Equal));

        let mut chain = vec![a];
        for (_, n) in cuts {
            if *chain.last().unwrap() != n {
                chain.push(n);
            }
        }
        if *chain.last().unwrap() != b {
            chain.push(b);
        }
        chain.windows(2).map(|w| self.link(w[0], w[1], lanes_fwd, lanes_bwd, kind)).collect()
    }

    /// Closest point on any alive street (paths excluded): where a car can be.
    pub fn nearest_street(&self, p: Vec2) -> Option<(SegmentId, f32, Vec2)> {
        let mut best: Option<(f32, SegmentId, f32, Vec2)> = None;
        for (sid, seg) in self.alive_segments() {
            if !seg.is_street() {
                continue;
            }
            let (p0, p1) = self.segment_points(sid);
            let (t, q) = project_on_segment(p, p0, p1);
            let d = p.distance(q);
            if best.map_or(true, |(bd, ..)| d < bd) {
                best = Some((d, sid, t, q));
            }
        }
        best.map(|(_, sid, t, q)| (sid, t, q))
    }

    /// Closest point on any alive segment.
    pub fn nearest_on_network(&self, p: Vec2) -> Option<(SegmentId, f32, Vec2)> {
        let mut best: Option<(f32, SegmentId, f32, Vec2)> = None;
        for (sid, _) in self.alive_segments() {
            let (p0, p1) = self.segment_points(sid);
            let (t, q) = project_on_segment(p, p0, p1);
            let d = p.distance(q);
            if best.map_or(true, |(bd, ..)| d < bd) {
                best = Some((d, sid, t, q));
            }
        }
        best.map(|(_, sid, t, q)| (sid, t, q))
    }

    /// Endpoint of `sid` closest to parameter `t`.
    pub fn nearest_endpoint(&self, sid: SegmentId, t: f32) -> NodeId {
        let s = self.segment(sid);
        if t < 0.5 {
            s.a
        } else {
            s.b
        }
    }

    pub fn neighbors(&self, n: NodeId) -> impl Iterator<Item = (NodeId, SegmentId)> + '_ {
        self.node(n).segments.iter().filter_map(move |&sid| {
            let s = self.segment(sid);
            if !s.alive {
                return None;
            }
            Some((if s.a == n { s.b } else { s.a }, sid))
        })
    }

    /// Nodes with a single segment: the city's entrances and dead ends.
    pub fn leaf_nodes(&self) -> Vec<NodeId> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.segments.iter().filter(|&&s| self.segment(s).alive).count() == 1)
            .map(|(i, _)| NodeId(i as u32))
            .collect()
    }

    /// A* over nodes. Returns the node sequence including both ends.
    pub fn path(&self, from: NodeId, to: NodeId) -> Option<Vec<NodeId>> {
        if from == to {
            return Some(vec![from]);
        }
        let goal = self.node(to).pos;
        let mut open = BinaryHeap::new();
        let mut came_from: HashMap<NodeId, NodeId> = HashMap::new();
        let mut g: HashMap<NodeId, f32> = HashMap::new();
        g.insert(from, 0.0);
        open.push(Open { f: self.node(from).pos.distance(goal), node: from });

        while let Some(Open { node, .. }) = open.pop() {
            if node == to {
                let mut path = vec![to];
                let mut cur = to;
                while let Some(&prev) = came_from.get(&cur) {
                    path.push(prev);
                    cur = prev;
                }
                path.reverse();
                return Some(path);
            }
            let gn = g[&node];
            for (next, _) in self.neighbors(node) {
                let cost = gn + self.node(node).pos.distance(self.node(next).pos);
                if g.get(&next).map_or(true, |&old| cost < old) {
                    g.insert(next, cost);
                    came_from.insert(next, node);
                    open.push(Open { f: cost + self.node(next).pos.distance(goal), node: next });
                }
            }
        }
        None
    }
}

#[derive(PartialEq)]
struct Open {
    f: f32,
    node: NodeId,
}

impl Eq for Open {}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap; invert so the smallest f pops first.
        other.f.total_cmp(&self.f).then_with(|| other.node.cmp(&self.node))
    }
}

/// Parameters `(t, u)` of the intersection of `p0p1` and `q0q1`, when it is
/// strictly inside both segments.
pub fn segment_intersection(p0: Vec2, p1: Vec2, q0: Vec2, q1: Vec2) -> Option<(f32, f32)> {
    let r = p1 - p0;
    let s = q1 - q0;
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-6 {
        return None;
    }
    let qp = q0 - p0;
    let t = qp.perp_dot(s) / denom;
    let u = qp.perp_dot(r) / denom;
    let eps = 1e-4;
    if t > eps && t < 1.0 - eps && u > eps && u < 1.0 - eps {
        Some((t, u))
    } else {
        None
    }
}

/// Project `p` onto segment `ab`; returns `(t, point)`.
pub fn project_on_segment(p: Vec2, a: Vec2, b: Vec2) -> (f32, Vec2) {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1e-6 {
        return (0.0, a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (t, a + ab * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_roads_make_an_intersection_node() {
        let mut net = RoadNetwork::default();
        net.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
        net.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
        assert_eq!(net.nodes.len(), 5);
        assert_eq!(net.alive_segments().count(), 4);
        let from = net.node_near(Vec2::new(-100.0, 0.0), 1.0).unwrap();
        let to = net.node_near(Vec2::new(0.0, 100.0), 1.0).unwrap();
        let path = net.path(from, to).unwrap();
        assert_eq!(path.len(), 3);
    }

    #[test]
    fn grid_is_fully_connected() {
        let mut net = RoadNetwork::default();
        for i in -2..=2 {
            let c = i as f32 * 100.0;
            net.add_polyline(&[Vec2::new(-200.0, c), Vec2::new(200.0, c)], 1, 1);
            net.add_polyline(&[Vec2::new(c, -200.0), Vec2::new(c, 200.0)], 1, 1);
        }
        let a = net.node_near(Vec2::new(-200.0, -200.0), 1.0).unwrap();
        let b = net.node_near(Vec2::new(200.0, 200.0), 1.0).unwrap();
        assert!(net.path(a, b).is_some());
    }

    #[test]
    fn road_end_dropped_near_a_road_snaps_and_makes_a_junction() {
        let mut net = RoadNetwork::default();
        net.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        // The end lands 6 m off the centreline, well inside the snap radius.
        net.add_polyline(&[Vec2::new(50.0, 150.0), Vec2::new(50.0, 6.0)], 1, 1);
        let junction = net.nodes.iter().position(|n| n.pos.distance(Vec2::new(50.0, 0.0)) < 0.5).expect("node on the road");
        let alive = net.nodes[junction].segments.iter().filter(|&&s| net.segment(s).alive).count();
        assert_eq!(alive, 3, "the long road is split at the snapped end");
        assert_eq!(net.alive_segments().count(), 3);
        // Far away ends stay free.
        net.add_polyline(&[Vec2::new(-50.0, 150.0), Vec2::new(-50.0, 40.0)], 1, 1);
        assert!(net.nodes.iter().any(|n| n.pos.distance(Vec2::new(-50.0, 40.0)) < 0.5));
    }

    #[test]
    fn snap_point_prefers_nodes_then_roads() {
        let mut net = RoadNetwork::default();
        net.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 2, 2);
        let (p, snapped) = net.snap_point(Vec2::new(30.0, 9.0), 8.0);
        assert!(snapped && p.distance(Vec2::new(30.0, 0.0)) < 0.01);
        let (p, snapped) = net.snap_point(Vec2::new(196.0, 5.0), 8.0);
        assert!(snapped && p == Vec2::new(200.0, 0.0));
        let (_, snapped) = net.snap_point(Vec2::new(30.0, 60.0), 8.0);
        assert!(!snapped);
    }
}
