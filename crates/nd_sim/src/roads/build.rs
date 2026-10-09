//! Building the network: polylines laid as segments, their ends snapped
//! onto nodes and roads, crossings split into junctions.

use super::*;

/// [interchange] Where a dropped road end lands (`RoadNetwork::end_target`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum EndTarget {
    /// On an existing node.
    Node(NodeId),
    /// On a way, at this parameter and point (it is split there).
    On(SegmentId, f32, Vec2),
}

impl RoadNetwork {
    /// May a way at `level` (a highway or not) share node `n`? A node with
    /// no ways yet takes anything; otherwise some way there must be one it
    /// could meet at a junction (not grade-separated from it).
    pub(super) fn node_accepts(&self, n: NodeId, level: f32, class: WayClass) -> bool {
        let node = self.node(n);
        let mut alive = node.segments.iter().map(|&s| self.segment(s)).filter(|s| s.alive).peekable();
        if alive.peek().is_none() {
            return true;
        }
        alive.any(|s| !separated(s.class(), node.level, class, level))
    }

    pub(super) fn push_node(&mut self, p: Vec2, level: f32) -> NodeId {
        self.nodes.push(Node { pos: p, segments: Vec::new(), level });
        NodeId((self.nodes.len() - 1) as u32)
    }

    /// Node for a junction where a way at `level` crosses another at grade:
    /// a node of any kind nearby that the way may meet, else a new one.
    pub(super) fn get_or_add_node(&mut self, p: Vec2, level: f32, class: WayClass) -> NodeId {
        let near = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (NodeId(i as u32), n.pos.distance(p)))
            .filter(|&(id, d)| d <= SNAP_RADIUS && self.node_accepts(id, level, class))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match near {
            Some((id, _)) => id,
            None => self.push_node(p, level),
        }
    }

    /// Node for an interior vertex: only a node its kind can join at its
    /// level, so a street vertex passing near the track does not become a
    /// crossing, nor a viaduct vertex over a street a junction.
    pub(super) fn get_or_add_node_kind(&mut self, p: Vec2, kind: RoadKind, level: f32, class: WayClass) -> NodeId {
        if let Some(id) = self.node_near_kind_where(p, SNAP_RADIUS, kind, |net, n| net.node_accepts(n, level, class)) {
            return id;
        }
        // [interchange] A vertex right on a node (the tool snaps a click to
        // it) meets it deliberately: a street's control point on the end of
        // a ramp is the ramp's terminal junction, though the street only
        // passes slip roads it crosses.
        if let Some(id) = self.node_near_kind_where(p, SNAP_EXACT, kind, |net, n| net.node_accepts(n, level, class.snapped())) {
            return id;
        }
        // A vertex lying on a way it may meet (a height sample of one road
        // falling on another) splits it there: a crossing exactly at a
        // vertex is no crossing to the segment test.
        // A street's vertex lying on the track at grade is a level crossing
        // there ([road-polish]: the crossing test between pieces does not
        // see a crossing at a piece's end, so the street used to pass under
        // the track at grade, the track drawn floating over its lanes).
        let level_crossing = |s: &Segment| s.kind == RoadKind::Rail && kind == RoadKind::Street;
        let on = self.nearest_where(p, |s| s.alive).filter(|&(sid, t, q)| {
            let s = self.segment(sid);
            q.distance(p) < 0.5 && ((s.kind == RoadKind::Rail) == (kind == RoadKind::Rail) || level_crossing(s)) && !separated(s.class(), self.segment_level(sid, t), class.snapped(), level)
        });
        if let Some((sid, _, _)) = on {
            // On one of the track's own nodes: cross there.
            let s = self.segment(sid);
            if level_crossing(s) {
                for n in [s.a, s.b] {
                    if self.node(n).pos.distance(p) < 0.5 {
                        return n;
                    }
                }
            }
        }
        let id = self.push_node(p, level);
        if let Some((sid, _, _)) = on {
            self.split_segment(sid, id);
        }
        id
    }

    /// Node for a road end: an existing node nearby, else a new node on the
    /// nearest road within `radius` (splitting it into a junction), else a
    /// fresh node at `p` at `level`. A dropped end joins whatever it was
    /// dropped on, at that road's level (that is how ramps meet highways).
    /// `own` are the pieces already laid for this same way: its far end
    /// never snaps back onto them (a curve's last sample is closer to the
    /// one before it than the snap radius). See [`RoadNetwork::end_target`]
    /// for which way it picks.
    pub(super) fn end_node(&mut self, p: Vec2, radius: f32, kind: RoadKind, level: f32, own: &[SegmentId]) -> NodeId {
        // Its own start stays a target, so a closed loop (a ring) closes.
        let start = own.first().map(|&s| self.segment(s).a);
        let own_nodes: Vec<NodeId> = own.iter().flat_map(|&s| [self.segment(s).a, self.segment(s).b]).filter(|&n| Some(n) != start).collect();
        match self.end_target(p, radius, kind, level, &own_nodes) {
            Some(EndTarget::Node(id)) => id,
            Some(EndTarget::On(sid, t, q)) => {
                let at = self.segment_level(sid, t);
                let id = self.push_node(q, at);
                self.split_segment(sid, id);
                id
            }
            None => self.push_node(p, level),
        }
    }

    /// [interchange] Where an end of a way of `kind` drawn at `level` and
    /// dropped at `p` lands: a node within `SNAP_RADIUS`, else a point on a
    /// way within `radius` (a node within `SNAP_RADIUS` of that point if
    /// there is one), else nowhere. Of what is in reach, a way at about the
    /// end's own level (`GRADE_SEPARATION`) wins: an end dropped where a
    /// viaduct passes over a street joins the one it was drawn at, and a
    /// ramp end at a street under the highway lands on the street, never on
    /// the deck overhead (it used to take the nearest node of any level).
    /// Only with nothing at its level in reach does it come up or down to
    /// a way at another level (a raised road ramps down to the road it is
    /// dropped on). A path lands on the highway or a slip road only with no
    /// other way in reach (a footpath dropped by an on-ramp beside the
    /// street the ramp leaves used to split the ramp, leading walkers onto
    /// it; it now joins the street).
    pub(super) fn end_target(&self, p: Vec2, radius: f32, kind: RoadKind, level: f32, own_nodes: &[NodeId]) -> Option<EndTarget> {
        let foot = kind == RoadKind::Path;
        let seg_ok = |s: &Segment| s.alive && Self::joins(s.kind, kind) && !(own_nodes.contains(&s.a) || own_nodes.contains(&s.b));
        // A node is a target when some way of it may take the end (for a
        // path on the walkers' pass, a way that is not a motorway).
        let node_ok = |net: &Self, n: NodeId, walk: bool| {
            if own_nodes.contains(&n) {
                return false;
            }
            let node = net.node(n);
            let mut alive = node.segments.iter().map(|&s| net.segment(s)).filter(|s| s.alive).peekable();
            alive.peek().is_none() || alive.any(|s| Self::joins(s.kind, kind) && !(walk && s.is_motorway()))
        };
        let at_level = |l: f32| (l - level).abs() < GRADE_SEPARATION;
        let node_level = |net: &Self, n: NodeId| {
            let node = net.node(n);
            if node.segments.iter().any(|&s| net.segment(s).alive) {
                Some(node.level)
            } else {
                None
            }
        };
        // Ways within reach, nearest first: (distance, segment, t, point).
        let lo = p - Vec2::splat(radius);
        let hi = p + Vec2::splat(radius);
        let mut ways: Vec<(f32, SegmentId, f32, Vec2)> = self
            .segments_in_box(lo, hi)
            .filter(|&sid| seg_ok(self.segment(sid)))
            .map(|sid| {
                let (a, b) = self.segment_points(sid);
                let (t, q) = project_on_segment(p, a, b);
                (q.distance(p), sid, t, q)
            })
            .filter(|w| w.0 <= radius)
            .collect();
        ways.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
        // Passes: at its own level first; for a path, a way that is not
        // the highway or a slip road first (it lands on one only with
        // nothing else in reach; the walk graph stops short of it).
        let passes: &[(bool, bool)] = if foot { &[(true, true), (true, false), (false, true), (false, false)] } else { &[(true, false), (false, false)] };
        for &(same_level, walk) in passes {
            let level_ok = |l: Option<f32>| l.is_none_or(|l| !same_level || at_level(l));
            if let Some(id) = self.node_near_kind_where(p, SNAP_RADIUS, kind, |net, n| node_ok(net, n, walk) && level_ok(node_level(net, n))) {
                return Some(EndTarget::Node(id));
            }
            if let Some(&(_, sid, t, q)) = ways.iter().find(|w| level_ok(Some(self.segment_level(w.1, w.2))) && !(walk && self.segment(w.1).is_motorway())) {
                let l = self.segment_level(sid, t);
                if let Some(id) = self.node_near_kind_where(q, SNAP_RADIUS, kind, |net, n| node_ok(net, n, walk) && node_level(net, n).is_none_or(|nl| (nl - l).abs() < GRADE_SEPARATION)) {
                    return Some(EndTarget::Node(id));
                }
                return Some(EndTarget::On(sid, t, q));
            }
        }
        None
    }

    /// [tools] Where an end of a way dropped at `p` lands and the level it
    /// takes there, as `add_polyline_levels` lays it (`end_node`): a node
    /// within `SNAP_RADIUS` (its level), else a point on a road within
    /// `END_SNAP_RADIUS` (that road's level there), else None (the end stays
    /// where and how it was drawn). A raised road dropped on a road at grade
    /// ends on that road: its deck ramps down to it.
    pub fn end_landing(&self, p: Vec2, kind: RoadKind) -> Option<(Vec2, f32)> {
        self.end_landing_at(p, kind, 0.0)
    }

    /// [interchange] [`RoadNetwork::end_landing`] for an end drawn at
    /// `level`: a way at about that level wins over one at another
    /// (`end_target`).
    pub fn end_landing_at(&self, p: Vec2, kind: RoadKind, level: f32) -> Option<(Vec2, f32)> {
        match self.end_target(p, END_SNAP_RADIUS, kind, level, &[])? {
            EndTarget::Node(id) => {
                let n = self.node(id);
                n.segments.iter().any(|&s| self.segment(s).alive).then_some((n.pos, n.level))
            }
            EndTarget::On(sid, t, q) => Some((q, self.segment_level(sid, t))),
        }
    }

    /// [tools] A way's points and levels as the network lays it: each end
    /// that lands on a road (`end_landing`) at that road's point and level.
    pub fn as_built(&self, points: &[Vec2], levels: &[f32], kind: RoadKind) -> (Vec<Vec2>, Vec<f32>) {
        let mut pts = points.to_vec();
        let mut lv: Vec<f32> = (0..points.len()).map(|i| levels.get(i).copied().unwrap_or(0.0)).collect();
        let n = pts.len();
        if n >= 2 {
            for i in [0, n - 1] {
                if let Some((q, l)) = self.end_landing_at(pts[i], kind, lv[i]) {
                    pts[i] = q;
                    lv[i] = l;
                }
            }
        }
        (pts, lv)
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

    /// The level a road end dropped at `p` would take: the level of the
    /// node or road it snaps onto, if any.
    pub fn snap_level(&self, p: Vec2, radius: f32, kind: RoadKind) -> Option<f32> {
        if let Some(id) = self.node_near_kind(p, radius, kind) {
            return Some(self.node(id).level);
        }
        let (sid, t, q) = self.nearest_of_kind(p, kind)?;
        (q.distance(p) <= self.segment(sid).half_width() + radius).then(|| self.segment_level(sid, t))
    }

    /// Whether ways of two kinds may share an end node.
    pub fn joins(a: RoadKind, b: RoadKind) -> bool {
        (a == RoadKind::Rail) == (b == RoadKind::Rail)
    }

    /// Nearest node within `radius` that carries a way `kind` can join
    /// (a node with no ways yet joins anything).
    pub(super) fn node_near_kind(&self, p: Vec2, radius: f32, kind: RoadKind) -> Option<NodeId> {
        self.node_near_kind_where(p, radius, kind, |_, _| true)
    }

    pub(super) fn node_near_kind_where(&self, p: Vec2, radius: f32, kind: RoadKind, keep: impl Fn(&Self, NodeId) -> bool) -> Option<NodeId> {
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
            if best.is_some_and(|(bd, _)| d >= bd) || !keep(self, NodeId(i as u32)) {
                continue;
            }
            best = Some((d, NodeId(i as u32)));
        }
        best.map(|(_, id)| id)
    }

    pub(super) fn find_link(&self, a: NodeId, b: NodeId) -> Option<SegmentId> {
        self.node(a).segments.iter().copied().find(|&sid| {
            let s = self.segment(sid);
            s.alive && ((s.a == a && s.b == b) || (s.a == b && s.b == a))
        })
    }

    pub(super) fn link(&mut self, a: NodeId, b: NodeId, lanes_fwd: u8, lanes_bwd: u8, kind: RoadKind) -> SegmentId {
        if let Some(existing) = self.find_link(a, b) {
            return existing;
        }
        self.index.take();
        self.segments.push(Segment { a, b, lanes_fwd, lanes_bwd, alive: true, kind, bridge: false, viaduct: false, tunnel: false, ring: self.laying_ring, ramp: false });
        let id = SegmentId((self.segments.len() - 1) as u32);
        self.nodes[a.0 as usize].segments.push(id);
        self.nodes[b.0 as usize].segments.push(id);
        id
    }

    pub(super) fn split_segment(&mut self, sid: SegmentId, at: NodeId) {
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
            self.segments[id.0 as usize].viaduct = s.viaduct;
            self.segments[id.0 as usize].tunnel = s.tunnel;
            self.segments[id.0 as usize].ring = s.ring;
            self.segments[id.0 as usize].ramp = s.ramp;
        }
    }

    /// Add a road along `points`. Crossing existing segments splits them so
    /// intersections become real nodes. Returns the created segments.
    pub fn add_polyline(&mut self, points: &[Vec2], lanes_fwd: u8, lanes_bwd: u8) -> Vec<SegmentId> {
        self.add_polyline_kind(points, lanes_fwd, lanes_bwd, RoadKind::Street)
    }

    /// Add a road or path along `points`, at grade.
    pub fn add_polyline_kind(&mut self, points: &[Vec2], lanes_fwd: u8, lanes_bwd: u8, kind: RoadKind) -> Vec<SegmentId> {
        self.add_polyline_levels(points, &[], lanes_fwd, lanes_bwd, kind)
    }

    /// Add a way along `points` with a level per point (metres above the
    /// ground; missing levels are 0). Ways it crosses at about its level
    /// are split into junctions; ways it crosses well above or below it
    /// (`grade_separated`) are not: the upper of the two gets a vertex at
    /// the crossing instead, so it can be lifted clear of the lower.
    pub fn add_polyline_levels(&mut self, points: &[Vec2], levels: &[f32], lanes_fwd: u8, lanes_bwd: u8, kind: RoadKind) -> Vec<SegmentId> {
        let mut out = Vec::new();
        let last = points.len().saturating_sub(1);
        self.laying_ring = self.is_ring_way(points, lanes_fwd, lanes_bwd, kind);
        let class = WayClass::laying(kind, lanes_fwd, lanes_bwd, self.laying_ring);
        let level = |i: usize| levels.get(i).copied().filter(|l| l.is_finite()).unwrap_or(0.0);
        for (i, w) in points.windows(2).enumerate() {
            if w[0].distance(w[1]) < 0.5 {
                continue;
            }
            // Road ends snap onto whatever road they were dropped near so a
            // T-junction forms; interior points only split roads they cross.
            let a = if i == 0 { self.end_node(w[0], END_SNAP_RADIUS, kind, level(0), &[]) } else { self.get_or_add_node_kind(w[0], kind, level(i), class) };
            let b = if i + 1 == last { self.end_node(w[1], END_SNAP_RADIUS, kind, level(i + 1), &out) } else { self.get_or_add_node_kind(w[1], kind, level(i + 1), class) };
            out.extend(self.add_segment_between(a, b, lanes_fwd, lanes_bwd, kind));
        }
        self.laying_ring = false;
        if !out.is_empty() {
            self.version += 1;
            self.classify();
        }
        out
    }

    pub(super) fn add_segment_between(&mut self, a: NodeId, b: NodeId, lanes_fwd: u8, lanes_bwd: u8, kind: RoadKind) -> Vec<SegmentId> {
        if a == b {
            return Vec::new();
        }
        let pa = self.node(a).pos;
        let pb = self.node(b).pos;
        let (la, lb) = (self.node(a).level, self.node(b).level);
        let level_at = |t: f32| la + (lb - la) * t;

        let mut cuts: Vec<(f32, NodeId)> = Vec::new();
        // Only segments whose box touches the new piece can cross it. The
        // candidates are fixed before splitting starts, in ascending id
        // order, so the junction nodes come out as a full scan makes them.
        let candidates = self.grid().in_box(pa.min(pb), pa.max(pb));
        let class = WayClass::laying(kind, lanes_fwd, lanes_bwd, self.laying_ring);
        for i in candidates {
            let s = self.segments[i as usize].clone();
            if !s.alive || s.a == a || s.a == b || s.b == a || s.b == b {
                continue;
            }
            let (p0, p1) = (self.node(s.a).pos, self.node(s.b).pos);
            if let Some((t, u)) = segment_intersection(pa, pb, p0, p1) {
                let x = pa.lerp(pb, t);
                let (mine, theirs) = (level_at(t), self.segment_level(SegmentId(i), u));
                if separated(class, mine, s.class(), theirs) {
                    // A flyover: no junction. The upper way gets a vertex of
                    // its own over the crossing (never shared: it is not
                    // snapped to the lower way's nodes).
                    if passes_over_class(class, mine, s.class(), theirs) {
                        let n = self.push_node(x, mine);
                        cuts.push((t, n));
                    } else {
                        let n = self.push_node(x, theirs);
                        self.split_segment(SegmentId(i), n);
                    }
                    continue;
                }
                let n = self.get_or_add_node(x, mine, class);
                if n != s.a && n != s.b {
                    self.split_segment(SegmentId(i), n);
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
                // Only a node this way could meet: not one on a highway (or
                // a street, for a highway), nor on a slip road for a street
                // or a path, nor one well above or below.
                if !self.node_accepts(id, level_at(t), class) || n.segments.iter().any(|&s| self.segments[s.0 as usize].alive && separated(self.segments[s.0 as usize].class(), 0.0, class, 0.0)) {
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
}
