//! Road network: a node/segment graph with snapping, intersection splitting
//! and A* pathfinding.

use crate::geom::Grid;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

pub use crate::geom::project_on_segment;

/// Cell size of the segment index. Road pieces are 7 m (curves) to a few
/// hundred metres (straight roads); a nearest query reads a 3x3 block of
/// cells in the common case.
const SEGMENT_CELL: f32 = 40.0;

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
    /// A railway: trains only, single track.
    Rail,
}

/// Whether a way with these lanes is a highway carriageway (see
/// [`Segment::is_highway`]).
pub fn is_highway_spec(kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8) -> bool {
    kind == RoadKind::Street && (lanes_fwd == 0 || lanes_bwd == 0) && lanes_fwd + lanes_bwd >= 3
}

/// Half the width of a path, in metres.
/// Half the corridor of a single track: ballast shoulder to ballast shoulder.
pub const RAIL_HALF_WIDTH: f32 = 2.4;
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
    /// Carried over water on a deck (set by the world from the landscape
    /// when the network is rebuilt).
    #[serde(default)]
    pub bridge: bool,
}

impl Segment {
    pub fn lanes(&self) -> u8 {
        self.lanes_fwd + self.lanes_bwd
    }

    pub fn is_street(&self) -> bool {
        self.kind == RoadKind::Street
    }

    pub fn is_rail(&self) -> bool {
        self.kind == RoadKind::Rail
    }

    /// A one-way street of three or more lanes: a highway carriageway. It is
    /// fast, fronts no lots, and is grade-separated: other ways cross it
    /// without a junction unless an end is dropped on it (a ramp).
    pub fn is_highway(&self) -> bool {
        is_highway_spec(self.kind, self.lanes_fwd, self.lanes_bwd)
    }

    pub fn half_width(&self) -> f32 {
        match self.kind {
            RoadKind::Street => self.lanes() as f32 * LANE_WIDTH * 0.5 + SHOULDER,
            RoadKind::Path => PATH_HALF_WIDTH,
            RoadKind::Rail => RAIL_HALF_WIDTH,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RoadNetwork {
    pub nodes: Vec<Node>,
    pub segments: Vec<Segment>,
    /// Incremented on every change so renderers can cheaply detect updates.
    pub version: u64,
    /// Grid over the alive segments, built on the first nearest query after
    /// a change and dropped by the next change. Every mutation goes through
    /// `link` or `split_segment`, which reset it.
    #[serde(skip)]
    index: OnceLock<Grid>,
    /// Roundabouts built into the network (set by the world's rebuild):
    /// their ring pieces are one-way streets on the circle.
    #[serde(default)]
    pub rings: Vec<Ring>,
}

impl RoadNetwork {
    /// The segment index, built if a change dropped it. Items are segment
    /// indices; dead segments get an empty box and are never returned.
    fn grid(&self) -> &Grid {
        self.index.get_or_init(|| {
            Grid::new(
                SEGMENT_CELL,
                self.segments.iter().map(|s| {
                    if s.alive {
                        let (a, b) = (self.node(s.a).pos, self.node(s.b).pos);
                        (a.min(b), a.max(b))
                    } else {
                        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN))
                    }
                }),
            )
        })
    }

    /// Closest point on any alive segment that passes `keep`. Exact ties go
    /// to the lower segment id, as the linear scan this replaces did.
    pub fn nearest_where(&self, p: Vec2, keep: impl Fn(&Segment) -> bool) -> Option<(SegmentId, f32, Vec2)> {
        let (i, _) = self.grid().nearest(p, |i| {
            let seg = &self.segments[i as usize];
            if !seg.alive || !keep(seg) {
                return f32::INFINITY;
            }
            let (a, b) = (self.node(seg.a).pos, self.node(seg.b).pos);
            p.distance(project_on_segment(p, a, b).1)
        })?;
        let sid = SegmentId(i);
        let (a, b) = self.segment_points(sid);
        let (t, q) = project_on_segment(p, a, b);
        Some((sid, t, q))
    }
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    pub fn segment(&self, id: SegmentId) -> &Segment {
        &self.segments[id.0 as usize]
    }

    /// A segment by id, or None when the id is from another network (ids
    /// are renumbered by every rebuild) or the segment is gone.
    pub fn try_segment(&self, id: SegmentId) -> Option<&Segment> {
        self.segments.get(id.0 as usize).filter(|s| s.alive)
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

    /// Node for an interior vertex: only a node its kind can join, so a
    /// street vertex passing near the track does not become a crossing.
    fn get_or_add_node_kind(&mut self, p: Vec2, kind: RoadKind) -> NodeId {
        if let Some(id) = self.node_near_kind(p, SNAP_RADIUS, kind) {
            return id;
        }
        self.nodes.push(Node { pos: p, segments: Vec::new() });
        NodeId((self.nodes.len() - 1) as u32)
    }

    /// Node for a road end: an existing node nearby, else a new node on the
    /// nearest road within `radius` (splitting it into a junction), else a
    /// fresh node at `p`.
    fn end_node(&mut self, p: Vec2, radius: f32, kind: RoadKind) -> NodeId {
        if let Some(id) = self.node_near_kind(p, SNAP_RADIUS, kind) {
            return id;
        }
        if let Some((sid, _t, q)) = self.nearest_of_kind(p, kind) {
            if q.distance(p) <= radius {
                if let Some(id) = self.node_near_kind(q, SNAP_RADIUS, kind) {
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
        self.snap_point_kind(p, radius, RoadKind::Street)
    }

    /// Track ends join track; road and path ends join roads and paths.
    pub fn snap_point_kind(&self, p: Vec2, radius: f32, kind: RoadKind) -> (Vec2, bool) {
        if let Some(id) = self.node_near_kind(p, radius, kind) {
            return (self.node(id).pos, true);
        }
        if let Some((sid, _, q)) = self.nearest_of_kind(p, kind) {
            if q.distance(p) <= self.segment(sid).half_width() + radius {
                return (q, true);
            }
        }
        (p, false)
    }

    /// Whether ways of two kinds may share an end node.
    pub fn joins(a: RoadKind, b: RoadKind) -> bool {
        (a == RoadKind::Rail) == (b == RoadKind::Rail)
    }

    /// Nearest node within `radius` that carries a way `kind` can join
    /// (a node with no ways yet joins anything).
    fn node_near_kind(&self, p: Vec2, radius: f32, kind: RoadKind) -> Option<NodeId> {
        let mut best: Option<(f32, NodeId)> = None;
        for (i, n) in self.nodes.iter().enumerate() {
            let d = n.pos.distance(p);
            if d > radius {
                continue;
            }
            let alive: Vec<_> = n.segments.iter().filter(|&&s| self.segment(s).alive).collect();
            if !alive.is_empty() && !alive.iter().any(|&&s| Self::joins(self.segment(s).kind, kind)) {
                continue;
            }
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, NodeId(i as u32)));
            }
        }
        best.map(|(_, id)| id)
    }

    /// Closest point on any alive way that `kind` can join.
    pub fn nearest_of_kind(&self, p: Vec2, kind: RoadKind) -> Option<(SegmentId, f32, Vec2)> {
        self.nearest_where(p, |seg| Self::joins(seg.kind, kind))
    }

    /// Closest point on the railway.
    pub fn nearest_rail(&self, p: Vec2) -> Option<(SegmentId, f32, Vec2)> {
        self.nearest_of_kind(p, RoadKind::Rail)
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
        self.index.take();
        self.segments.push(Segment { a, b, lanes_fwd, lanes_bwd, alive: true, kind, bridge: false });
        let id = SegmentId((self.segments.len() - 1) as u32);
        self.nodes[a.0 as usize].segments.push(id);
        self.nodes[b.0 as usize].segments.push(id);
        id
    }

    fn split_segment(&mut self, sid: SegmentId, at: NodeId) {
        let s = self.segments[sid.0 as usize].clone();
        self.index.take();
        self.segments[sid.0 as usize].alive = false;
        for n in [s.a, s.b] {
            self.nodes[n.0 as usize].segments.retain(|&x| x != sid);
        }
        let first = self.link(s.a, at, s.lanes_fwd, s.lanes_bwd, s.kind);
        let second = self.link(at, s.b, s.lanes_fwd, s.lanes_bwd, s.kind);
        for id in [first, second] {
            self.segments[id.0 as usize].bridge = s.bridge;
        }
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
            let a = if i == 0 { self.end_node(w[0], END_SNAP_RADIUS, kind) } else { self.get_or_add_node_kind(w[0], kind) };
            let b = if i + 1 == last { self.end_node(w[1], END_SNAP_RADIUS, kind) } else { self.get_or_add_node_kind(w[1], kind) };
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
        // Only segments whose box touches the new piece can cross it. The
        // candidates are fixed before splitting starts, in ascending id
        // order, so the junction nodes come out as a full scan makes them.
        let candidates = self.grid().in_box(pa.min(pb), pa.max(pb));
        let highway = is_highway_spec(kind, lanes_fwd, lanes_bwd);
        for i in candidates {
            let s = self.segments[i as usize].clone();
            if !s.alive || s.a == a || s.a == b || s.b == a || s.b == b {
                continue;
            }
            // Highways are grade-separated: crossing one is a flyover.
            if s.is_highway() != highway {
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
        // Crossings exactly at an existing node are skipped above (the
        // parametric test excludes the ends), so a way drawn through a
        // junction, or over a vertex of another way, joins it here.
        let len = pa.distance(pb);
        if len > SNAP_RADIUS * 2.0 {
            for i in 0..self.nodes.len() {
                let id = NodeId(i as u32);
                if id == a || id == b || cuts.iter().any(|c| c.1 == id) {
                    continue;
                }
                let n = &self.nodes[i];
                // Geometry first: it is the cheap test, and most nodes fail it.
                let (t, q) = project_on_segment(n.pos, pa, pb);
                let margin = SNAP_RADIUS / len;
                if !(t > margin && t < 1.0 - margin && q.distance(n.pos) <= SNAP_RADIUS) {
                    continue;
                }
                let mut alive = n.segments.iter().copied().filter(|&s| self.segments[s.0 as usize].alive).peekable();
                if alive.peek().is_none() || !alive.any(|s| Self::joins(self.segments[s.0 as usize].kind, kind)) {
                    continue;
                }
                if n.segments.iter().any(|&s| self.segments[s.0 as usize].alive && self.segments[s.0 as usize].is_highway() != highway) {
                    continue;
                }
                cuts.push((t, id));
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
        self.nearest_where(p, Segment::is_street)
    }

    /// Closest point on any alive street or path (never the railway).
    pub fn nearest_on_network(&self, p: Vec2) -> Option<(SegmentId, f32, Vec2)> {
        self.nearest_where(p, |seg| !seg.is_rail())
    }

    /// Alive segments whose bounding box touches `(lo, hi)`, ascending.
    pub fn segments_in_box(&self, lo: Vec2, hi: Vec2) -> impl Iterator<Item = SegmentId> + '_ {
        self.grid().in_box(lo, hi).into_iter().map(SegmentId).filter(move |&s| self.segments[s.0 as usize].alive)
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

    /// Nodes with a single street or path: the city's entrances and dead ends.
    pub fn leaf_nodes(&self) -> Vec<NodeId> {
        self.leaf_nodes_where(|s| !s.is_rail())
    }

    /// Ends of the railway: where trains enter and leave.
    pub fn rail_leaf_nodes(&self) -> Vec<NodeId> {
        self.leaf_nodes_where(|s| s.is_rail())
    }

    fn leaf_nodes_where(&self, keep: impl Fn(&Segment) -> bool) -> Vec<NodeId> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                let mine: Vec<_> = n.segments.iter().filter(|&&s| self.segment(s).alive && keep(self.segment(s))).collect();
                mine.len() == 1 && n.segments.iter().filter(|&&s| self.segment(s).alive).count() == 1
            })
            .map(|(i, _)| NodeId(i as u32))
            .collect()
    }

    /// A* over street and path nodes. Returns the node sequence including both ends.
    pub fn path(&self, from: NodeId, to: NodeId) -> Option<Vec<NodeId>> {
        self.path_where(from, to, |s| !s.is_rail())
    }

    /// A* along the railway only.
    pub fn rail_path(&self, from: NodeId, to: NodeId) -> Option<Vec<NodeId>> {
        self.path_where(from, to, |s| s.is_rail())
    }

    pub fn path_where(&self, from: NodeId, to: NodeId, keep: impl Fn(&Segment) -> bool) -> Option<Vec<NodeId>> {
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
            for (next, sid) in self.neighbors(node) {
                if !keep(self.segment(sid)) {
                    continue;
                }
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

impl RoadNetwork {
    /// The roundabout whose ring this segment is a piece of: a one-way
    /// street with both ends on the circle.
    pub fn ring_of_segment(&self, sid: SegmentId) -> Option<usize> {
        if self.rings.is_empty() {
            return None;
        }
        let s = self.segments.get(sid.0 as usize)?;
        if !s.alive || !s.is_street() || s.lanes_bwd != 0 || s.lanes_fwd == 0 {
            return None;
        }
        let (a, b) = (self.node(s.a).pos, self.node(s.b).pos);
        self.rings.iter().position(|r| s.lanes_fwd == r.lanes && r.on_circle(a) && r.on_circle(b))
    }

    /// The roundabout a node sits on (any node with a ring piece).
    pub fn ring_of_node(&self, n: NodeId) -> Option<usize> {
        if self.rings.is_empty() {
            return None;
        }
        self.node(n).segments.iter().find_map(|&s| self.ring_of_segment(s))
    }
}

// ── roundabouts ─────────────────────────────────────────────────────────────

/// Roundabout radius limits (centre to the middle of the ring road), metres:
/// 24 to 80 m across.
pub const RING_MIN_RADIUS: f32 = 12.0;
pub const RING_MAX_RADIUS: f32 = 40.0;
/// Two arms whose joins on the circle are closer than this share one node.
const RING_JOIN_MERGE: f32 = 8.0;
/// Target length of a ring piece between joins.
const RING_PIECE: f32 = 10.0;

/// A roundabout: a one-way ring road round a central island. Traffic keeps
/// the island on its left (right-hand traffic: anticlockwise seen from
/// above, which is clockwise in the sim plane's x/y, since y maps to
/// world z).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ring {
    pub center: Vec2,
    /// Centre of the island to the middle of the ring road.
    pub radius: f32,
    /// Ring lanes, 1 or 2.
    pub lanes: u8,
}

impl Ring {
    /// A ring with its radius and lanes brought within limits.
    pub fn new(center: Vec2, radius: f32, lanes: u8) -> Self {
        Self { center, radius: radius.clamp(RING_MIN_RADIUS, RING_MAX_RADIUS), lanes: lanes.clamp(1, 2) }
    }

    pub fn half_width(&self) -> f32 {
        self.lanes as f32 * LANE_WIDTH * 0.5 + SHOULDER
    }

    /// Radius of the raised central island (inside the ring's kerb).
    pub fn island_radius(&self) -> f32 {
        (self.radius - self.half_width() - 0.3).max(2.0)
    }

    /// Radius of the ground the ring takes, pavement included.
    pub fn outer_radius(&self) -> f32 {
        self.radius + self.half_width() + crate::zoning::SIDEWALK
    }

    /// Is `p` on the ring's circle (a ring vertex, or a point a road end
    /// split a ring piece at)?
    pub fn on_circle(&self, p: Vec2) -> bool {
        let d = p.distance(self.center);
        d > self.radius - 1.6 && d < self.radius + 0.6
    }

    pub fn overlaps(&self, other: &Ring) -> bool {
        self.center.distance(other.center) < self.outer_radius() + other.outer_radius()
    }

    /// Direction of travel at a point of the ring (island on the left).
    pub fn travel_dir(&self, p: Vec2) -> Vec2 {
        let r = (p - self.center).normalize_or_zero();
        Vec2::new(r.y, -r.x)
    }

    /// Split a polyline at the circle: the pieces outside (their ends on
    /// the circle where they were cut) and the pieces inside, in order.
    pub fn clip(&self, pts: &[Vec2]) -> (Vec<Vec<Vec2>>, Vec<Vec<Vec2>>) {
        let (c, r) = (self.center, self.radius);
        let inside = |p: Vec2| p.distance(c) < r;
        if pts.len() < 2 {
            return (vec![pts.to_vec()], Vec::new());
        }
        let mut outside_pieces: Vec<Vec<Vec2>> = Vec::new();
        let mut inside_pieces: Vec<Vec<Vec2>> = Vec::new();
        let mut cur: Vec<Vec2> = vec![pts[0]];
        let mut cur_in = inside(pts[0]);
        for w in pts.windows(2) {
            let (p, q) = (w[0], w[1]);
            // Circle crossings strictly inside the leg, in order.
            let d = q - p;
            let f = p - c;
            let (a, b, cc) = (d.dot(d), 2.0 * f.dot(d), f.dot(f) - r * r);
            let disc = b * b - 4.0 * a * cc;
            if a > 1e-6 && disc > 0.0 {
                let sq = disc.sqrt();
                for t in [(-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)] {
                    if t > 1e-4 && t < 1.0 - 1e-4 {
                        let x = p + d * t;
                        cur.push(x);
                        let piece = std::mem::replace(&mut cur, vec![x]);
                        if cur_in { inside_pieces.push(piece) } else { outside_pieces.push(piece) }
                        cur_in = !cur_in;
                    }
                }
            }
            cur.push(q);
        }
        if cur_in { inside_pieces.push(cur) } else { outside_pieces.push(cur) }
        outside_pieces.retain(|v| path_len(v) > 1.0);
        inside_pieces.retain(|v| v.len() >= 2);
        (outside_pieces, inside_pieces)
    }

    /// The ring road as a closed polyline (first point repeated last) in
    /// the direction of travel, with a vertex at every join (snapped
    /// together where two are closer than a few metres) and pieces of about
    /// ten metres between. Returns the polyline and the joins as placed.
    pub fn polyline(&self, joins: &[Vec2]) -> (Vec<Vec2>, Vec<Vec2>) {
        use std::f32::consts::TAU;
        let (c, r) = (self.center, self.radius);
        let ang = |p: Vec2| {
            let a = (p - c).y.atan2((p - c).x);
            if a < 0.0 { a + TAU } else { a }
        };
        let mut a: Vec<f32> = joins.iter().map(|&p| ang(p)).collect();
        a.sort_by(|x, y| y.total_cmp(x));
        // Merge joins closer than RING_JOIN_MERGE along the circle.
        let min_da = RING_JOIN_MERGE / r;
        let mut merged: Vec<f32> = Vec::new();
        for x in a {
            if merged.last().is_some_and(|&l| l - x < min_da) {
                continue;
            }
            merged.push(x);
        }
        if merged.len() > 1 && (merged[merged.len() - 1] + TAU - merged[0]) < min_da {
            merged.pop();
        }
        let at = |t: f32| c + Vec2::new(t.cos(), t.sin()) * r;
        let mut out: Vec<Vec2> = Vec::new();
        if merged.is_empty() {
            let n = ((TAU * r / RING_PIECE).round() as usize).max(8);
            for k in 0..=n {
                out.push(at(-TAU * k as f32 / n as f32));
            }
            return (out, Vec::new());
        }
        for i in 0..merged.len() {
            let a0 = merged[i];
            let a1 = if i + 1 < merged.len() { merged[i + 1] } else { merged[0] - TAU };
            let span = a0 - a1; // angles decrease along the ring
            let k = ((span * r / RING_PIECE).floor() as usize).max(1);
            for j in 0..k {
                out.push(at(a0 - span * j as f32 / k as f32));
            }
        }
        out.push(out[0]);
        (out, merged.into_iter().map(at).collect())
    }
}

fn path_len(pts: &[Vec2]) -> f32 {
    pts.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// Roads reshaped by roundabouts, as built.
#[derive(Clone, Debug, Default)]
pub struct RingLayout {
    /// Per ring: the ring, its closed polyline and where roads join it.
    pub rings: Vec<(Ring, Vec<Vec2>, Vec<Vec2>)>,
    /// Pieces of each input road outside every ring, by input index.
    pub kept: Vec<(usize, Vec<Vec2>)>,
    /// Parts of roads inside a ring: what the works take up.
    pub cut: Vec<(usize, Vec<Vec2>)>,
}

/// Lay roundabouts over roads: each road polyline with the parts inside a
/// ring cut away (the ends left on the circle join the ring), each ring
/// with a vertex at every join. `roads` carry whether a ring may cut them
/// (railways and grade-separated highways pass untouched). Rings that
/// overlap an earlier one are left out.
pub fn layout_rings(rings: &[Ring], roads: &[(Vec<Vec2>, bool)]) -> RingLayout {
    let mut chosen: Vec<Ring> = Vec::new();
    for r in rings {
        if !chosen.iter().any(|o| o.overlaps(r)) {
            chosen.push(*r);
        }
    }
    let mut out = RingLayout::default();
    let mut joins: Vec<Vec<Vec2>> = vec![Vec::new(); chosen.len()];
    for (i, (pts, clip)) in roads.iter().enumerate() {
        if !clip {
            out.kept.push((i, pts.clone()));
            continue;
        }
        let mut pieces = vec![pts.clone()];
        for (k, ring) in chosen.iter().enumerate() {
            let mut next = Vec::new();
            for p in pieces {
                let (outside, inside) = ring.clip(&p);
                for o in &outside {
                    for end in [o[0], *o.last().unwrap()] {
                        if (end.distance(ring.center) - ring.radius).abs() < 1e-2 {
                            joins[k].push(end);
                        }
                    }
                }
                out.cut.extend(inside.into_iter().map(|c| (i, c)));
                next.extend(outside);
            }
            pieces = next;
        }
        out.kept.extend(pieces.into_iter().map(|p| (i, p)));
    }
    // Rings get their vertices; arm ends move onto the vertex they share.
    for (k, ring) in chosen.iter().enumerate() {
        let (poly, placed) = ring.polyline(&joins[k]);
        for (_, pts) in out.kept.iter_mut() {
            let last = pts.len() - 1;
            for end in [0, last] {
                let p = pts[end];
                if (p.distance(ring.center) - ring.radius).abs() < 1e-2 {
                    if let Some(q) = placed.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))) {
                        pts[end] = *q;
                    }
                }
            }
        }
        out.rings.push((*ring, poly, placed));
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference every grid-backed query must match: a full scan that
    /// keeps the first best.
    fn linear_nearest(net: &RoadNetwork, p: Vec2, keep: impl Fn(&Segment) -> bool) -> Option<(SegmentId, f32, Vec2)> {
        let mut best: Option<(f32, SegmentId, f32, Vec2)> = None;
        for (sid, seg) in net.alive_segments() {
            if !keep(seg) {
                continue;
            }
            let (p0, p1) = net.segment_points(sid);
            let (t, q) = project_on_segment(p, p0, p1);
            let d = p.distance(q);
            if best.is_none_or(|(bd, ..)| d < bd) {
                best = Some((d, sid, t, q));
            }
        }
        best.map(|(_, sid, t, q)| (sid, t, q))
    }

    fn city_grid(half: i32, spacing: f32) -> RoadNetwork {
        let mut net = RoadNetwork::default();
        let ext = half as f32 * spacing;
        for i in -half..=half {
            let c = i as f32 * spacing;
            net.add_polyline(&[Vec2::new(-ext, c), Vec2::new(ext, c)], 1, 1);
            net.add_polyline(&[Vec2::new(c, -ext), Vec2::new(c, ext)], 1, 1);
        }
        net.add_polyline_kind(&[Vec2::new(-ext, -ext - 30.0), Vec2::new(ext, -ext - 30.0)], 0, 0, RoadKind::Rail);
        net.add_polyline_kind(&[Vec2::new(-ext - 20.0, 0.0), Vec2::new(-ext - 20.0, ext)], 0, 0, RoadKind::Path);
        net
    }

    #[test]
    fn grid_nearest_queries_match_the_linear_scan() {
        let net = city_grid(6, 90.0);
        assert!(net.alive_segments().count() > 150);
        let mut seed = 11u32;
        for k in 0..600 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let mut p = Vec2::new((seed >> 8) as f32 / 16_777_216.0 * 1400.0 - 700.0, (seed >> 20) as f32 / 4096.0 * 1400.0 - 700.0);
            // Every third query sits on a junction or a lane centreline,
            // where several segments tie exactly.
            if k % 3 == 0 {
                p = Vec2::new((p.x / 90.0).round() * 90.0, if k % 2 == 0 { (p.y / 90.0).round() * 90.0 } else { p.y });
            }
            assert_eq!(net.nearest_street(p), linear_nearest(&net, p, Segment::is_street), "street at {p:?}");
            assert_eq!(net.nearest_on_network(p), linear_nearest(&net, p, |s| !s.is_rail()), "network at {p:?}");
            assert_eq!(net.nearest_of_kind(p, RoadKind::Rail), linear_nearest(&net, p, |s| s.is_rail()), "rail at {p:?}");
            assert_eq!(net.nearest_of_kind(p, RoadKind::Path), linear_nearest(&net, p, |s| !s.is_rail()), "path at {p:?}");
        }
        // Far outside the city the answer is still the right edge segment.
        let far = Vec2::new(5000.0, -3000.0);
        assert_eq!(net.nearest_street(far), linear_nearest(&net, far, Segment::is_street));
        // An empty network has no nearest.
        assert!(RoadNetwork::default().nearest_street(Vec2::ZERO).is_none());
        // The index follows edits: a new road becomes the nearest at once.
        let mut net = net;
        net.add_polyline(&[Vec2::new(2000.0, 2000.0), Vec2::new(2100.0, 2000.0)], 1, 1);
        let p = Vec2::new(2050.0, 2010.0);
        assert_eq!(net.nearest_street(p), linear_nearest(&net, p, Segment::is_street));
        assert!(net.nearest_street(p).unwrap().2.distance(Vec2::new(2050.0, 2000.0)) < 1e-3);
    }

    /// Rebuild cost of a large city: `cargo test -p nd_sim --release -- --ignored nearest_bench`.
    #[test]
    #[ignore]
    fn nearest_bench() {
        let net = city_grid(40, 50.0);
        let n = net.alive_segments().count();
        let t0 = std::time::Instant::now();
        let mut acc = 0.0;
        for i in 0..20_000 {
            let p = Vec2::new((i % 300) as f32 * 13.7 - 2000.0, (i / 300) as f32 * 31.1 - 2000.0);
            acc += net.nearest_street(p).map_or(0.0, |(_, t, _)| t);
        }
        let grid = t0.elapsed();
        let t0 = std::time::Instant::now();
        for i in 0..20_000 {
            let p = Vec2::new((i % 300) as f32 * 13.7 - 2000.0, (i / 300) as f32 * 31.1 - 2000.0);
            acc += linear_nearest(&net, p, Segment::is_street).map_or(0.0, |(_, t, _)| t);
        }
        let linear = t0.elapsed();
        eprintln!("{n} segments, 20k queries: grid {grid:?}, linear {linear:?} ({acc})");
        assert!(grid < linear);
    }

    #[test]
    fn a_street_crossing_the_track_near_a_rail_node_becomes_a_level_crossing() {
        let mut net = RoadNetwork::default();
        net.add_polyline_kind(&[Vec2::new(-100.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)], 0, 0, RoadKind::Rail);
        // The street crosses the track 3 m from its middle node: within the
        // snap radius, so the crossing is that node rather than a second one
        // a step away.
        net.add_polyline(&[Vec2::new(3.0, -100.0), Vec2::new(3.0, 100.0)], 1, 1);
        let crossing = net.node_near(Vec2::ZERO, SNAP_RADIUS).expect("crossing node");
        let kinds: Vec<RoadKind> = net.neighbors(crossing).map(|(_, s)| net.segment(s).kind).collect();
        assert_eq!(kinds.len(), 4, "two rail and two street pieces meet at the crossing: {kinds:?}");
        assert_eq!(kinds.iter().filter(|k| **k == RoadKind::Rail).count(), 2);
        assert_eq!(net.nodes.iter().filter(|n| n.pos.distance(Vec2::ZERO) < SNAP_RADIUS).count(), 1, "no second node beside the crossing");
        // Both ways still run through: trains along the track, cars along the street.
        let (ra, rb) = (net.node_near(Vec2::new(-100.0, 0.0), 1.0).unwrap(), net.node_near(Vec2::new(100.0, 0.0), 1.0).unwrap());
        assert_eq!(net.rail_path(ra, rb).map(|p| p.len()), Some(3));
        let (sa, sb) = (net.node_near(Vec2::new(3.0, -100.0), 1.0).unwrap(), net.node_near(Vec2::new(3.0, 100.0), 1.0).unwrap());
        assert_eq!(net.path(sa, sb).map(|p| p.len()), Some(3));
        // The rail ends are still the only rail leaves.
        assert_eq!(net.rail_leaf_nodes().len(), 2);
    }

    #[test]
    fn a_self_crossing_polyline_splits_itself_at_the_crossing() {
        let mut net = RoadNetwork::default();
        // A figure of eight: the second half crosses the first.
        let pts = [Vec2::new(-100.0, -50.0), Vec2::new(100.0, 50.0), Vec2::new(100.0, -50.0), Vec2::new(-100.0, 50.0)];
        let made = net.add_polyline(&pts, 1, 1);
        assert!(made.len() >= 4, "the crossing splits a leg: {} pieces", made.len());
        let x = net.node_near(Vec2::ZERO, 1.0).expect("a node at the crossing");
        assert_eq!(net.neighbors(x).count(), 4);
        assert!(net.nodes.iter().all(|n| n.pos.is_finite()));
        let (a, b) = (net.node_near(pts[0], 1.0).unwrap(), net.node_near(pts[3], 1.0).unwrap());
        let path = net.path(a, b).expect("the ends connect");
        // Via the crossing is shorter than along the whole figure.
        assert_eq!(path.len(), 3);
        assert_eq!(net.leaf_nodes().len(), 2);
    }

    #[test]
    fn a_road_drawn_through_a_junction_joins_it() {
        let mut net = RoadNetwork::default();
        net.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
        net.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
        let origin = net.node_near(Vec2::ZERO, 1.0).unwrap();
        assert_eq!(net.node(origin).segments.len(), 4);
        net.add_polyline(&[Vec2::new(-100.0, -100.0), Vec2::new(100.0, 100.0)], 1, 1);
        assert_eq!(net.node(origin).segments.len(), 6, "the diagonal passes through the junction");
        let from = net.node_near(Vec2::new(100.0, 100.0), 1.0).unwrap();
        let to = net.node_near(Vec2::new(100.0, 0.0), 1.0).unwrap();
        assert!(net.path(from, to).is_some());
        // Through the bend of a polyline too.
        let mut net = RoadNetwork::default();
        net.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::ZERO, Vec2::new(100.0, 40.0)], 1, 1);
        net.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
        let bend = net.node_near(Vec2::ZERO, 1.0).unwrap();
        assert_eq!(net.node(bend).segments.len(), 4);
    }

    #[test]
    fn a_street_vertex_near_the_track_does_not_make_a_crossing() {
        let mut net = RoadNetwork::default();
        net.add_polyline_kind(&[Vec2::new(-100.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)], 0, 0, RoadKind::Rail);
        // A street bending 4 m from the track's middle node, never crossing it.
        net.add_polyline(&[Vec2::new(-100.0, 40.0), Vec2::new(0.0, 4.0), Vec2::new(100.0, 40.0)], 1, 1);
        let rail_mid = net.node_near(Vec2::ZERO, 1.0).unwrap();
        assert!(net.node(rail_mid).segments.iter().all(|&s| net.segment(s).is_rail()), "no street joins the rail node");
        let street_mid = net.node_near(Vec2::new(0.0, 4.0), 1.0).unwrap();
        assert_ne!(rail_mid, street_mid);
    }

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
