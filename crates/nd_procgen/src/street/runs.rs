//! Runs: chains of segments drawn as one road, the tapers where a road
//! changes width, and the polyline cuts the drawing passes share.

use super::*;

/// A chain of segments drawn as one road.
pub(super) struct Run {
    pub(super) points: Vec<Vec2>,
    /// The road node at each point.
    pub(super) nodes: Vec<NodeId>,
    pub(super) first: SegmentId,
    pub(super) lanes_fwd: u8,
    pub(super) lanes_bwd: u8,
    pub(super) kind: RoadKind,
    pub(super) start_node: NodeId,
    pub(super) end_node: NodeId,
    /// The segment at each end of the run (`segment_between` its first
    /// two nodes and its last two), worked out once.
    pub(super) end_segs: [Option<SegmentId>; 2],
}

/// A node whose roads all run underground, drawn with the tunnels. A node
/// with no live roads at all is not one (it is drawn with nothing).
pub(super) fn tunnel_node(roads: &RoadNetwork, n: NodeId) -> bool {
    let mut live = roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive).peekable();
    live.peek().is_some() && live.all(|&s| roads.segment(s).tunnel)
}

/// Roads of one kind at a node.
pub(super) fn degree_of(roads: &RoadNetwork, n: NodeId, kind: RoadKind) -> usize {
    roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive && roads.segment(s).kind == kind).count()
}


/// Oriented lanes of a segment when travelling from `from` to its far node.
pub(super) fn oriented(roads: &RoadNetwork, sid: SegmentId, from: NodeId) -> (NodeId, u8, u8) {
    let s = roads.segment(sid);
    if s.a == from {
        (s.b, s.lanes_fwd, s.lanes_bwd)
    } else {
        (s.a, s.lanes_bwd, s.lanes_fwd)
    }
}

/// The segment continuing past a bend node, if the node is a plain bend of
/// the same road kind.
pub(super) fn continuation(roads: &RoadNetwork, node: NodeId, from_seg: SegmentId, lanes_fwd: u8, lanes_bwd: u8) -> Option<SegmentId> {
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

pub(super) fn build_runs(roads: &RoadNetwork) -> Vec<Run> {
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
        let k = nodes.len();
        let end_segs = [segment_between(roads, nodes[0], nodes[1]), segment_between(roads, nodes[k - 1], nodes[k - 2])];
        runs.push(Run { points, nodes, first, lanes_fwd: run_f, lanes_bwd: run_b, kind: roads.segment(first).kind, start_node, end_node: node, end_segs });
    }
    runs
}

/// Metres of taper per metre of width gained or lost, and its bounds.
pub(super) const TAPER_PER_M: f32 = 9.0;
pub(super) const TAPER_MIN: f32 = 12.0;
pub(super) const TAPER_MAX: f32 = 45.0;

/// At a bend node `n` (two streets, `alive`) where the carriageway changes
/// width: the run of the narrower street, its path from `n` (as long as
/// the taper), and the half widths at `n` (the wider street's) and at the
/// taper's end. None where both are as wide, where the narrower is a town
/// street, or where either is a ring, tunnel, bridge or viaduct (decks and
/// bores have widths of their own).
pub(super) fn lane_taper_at(roads: &RoadNetwork, runs: &[Run], n: NodeId, alive: &[SegmentId]) -> Option<(usize, Vec<Vec2>, f32, f32)> {
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
pub(super) fn lane_taper(g: &mut Geometry, path: &[Vec2], hw0: f32, hw1: f32, asphalt: [f32; 4], edge: [f32; 4], barrier: Option<[f32; 4]>) {
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
pub(super) fn arc_points(points: &[Vec2], centre: Vec2, radius: f32) -> Vec<Vec2> {
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
pub(super) fn clear_along(path: &[Vec2], from: f32, limit: f32, offsets: &[f32], others: &[(Vec<Vec2>, f32)]) -> f32 {
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
pub(super) fn trim(points: &[Vec2], start: f32, end: f32) -> Vec<Vec2> {
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
