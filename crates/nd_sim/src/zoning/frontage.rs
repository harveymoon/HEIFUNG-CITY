//! Frontage: the road corridors cut out of the land, and which edges of a
//! piece lie along a road (and so may front a lot).

use super::*;

/// Chains of road pieces joined at plain two-way nodes, with the corridor
/// half-width of the chain and whether each end sits at a junction or dead
/// end (where the corridor is extended to clear the corner).
pub(super) fn road_chains(roads: &RoadNetwork) -> Vec<(Vec<Vec2>, f32, bool, bool, SegmentId)> {
    let degree = |n: NodeId| roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive).count();
    let hw_of = |s: SegmentId| {
        let seg = roads.segment(s);
        if seg.is_street() { seg.half_width() + SIDEWALK } else { seg.half_width() + 1.0 }
    };
    let next_of = |node: NodeId, from: SegmentId| -> Option<SegmentId> {
        if degree(node) != 2 {
            return None;
        }
        let other = roads.node(node).segments.iter().copied().find(|&s| s != from && roads.segment(s).alive)?;
        ((hw_of(other) - hw_of(from)).abs() < 0.01 && roads.segment(other).tunnel == roads.segment(from).tunnel && roads.spans_lots(other) == roads.spans_lots(from)).then_some(other)
    };
    let far = |s: SegmentId, from: NodeId| { let seg = roads.segment(s); if seg.a == from { seg.b } else { seg.a } };
    let mut used = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (sid, seg) in roads.alive_segments() {
        // Tunnels take no ground: the parcels above them are whole. Nor
        // does a viaduct high enough to span over them (its columns find
        // room between the buildings).
        if used.contains(&sid.0) || seg.tunnel || roads.spans_lots(sid) {
            continue;
        }
        // Walk back to the chain start.
        let (mut cur, mut node) = (sid, seg.a);
        let mut guard = 0;
        while let Some(prev) = next_of(node, cur) {
            if prev == sid || guard > 10_000 {
                break;
            }
            guard += 1;
            node = far(prev, node);
            cur = prev;
        }
        let start = node;
        let mut pts = vec![roads.node(node).pos];
        let mut s = cur;
        loop {
            used.insert(s.0);
            node = far(s, node);
            pts.push(roads.node(node).pos);
            match next_of(node, s) {
                Some(n) if !used.contains(&n.0) => s = n,
                _ => break,
            }
        }
        let end_junction = degree(node) != 2 || node == start;
        let start_junction = degree(start) != 2 || node == start;
        out.push((pts, hw_of(cur), start_junction, end_junction, cur));
    }
    out
}

/// Corridor polygon of a chain: both sides offset with mitred joins (the
/// shared [`geom::MITRE_LIMIT`] policy) so a curve's outline is smooth, ends
/// extended by the half-width at junctions and dead ends so the corners get
/// cleared.
pub(super) fn chain_corridor(pts: &[Vec2], hw: f32, ext_start: bool, ext_end: bool) -> Option<Polygon<f64>> {
    if pts.len() < 2 {
        return None;
    }
    let mut p = pts.to_vec();
    let n = p.len();
    let d0 = (p[1] - p[0]).normalize_or_zero();
    let d1 = (p[n - 1] - p[n - 2]).normalize_or_zero();
    p[0] -= d0 * if ext_start { hw } else { 0.5 };
    p[n - 1] += d1 * if ext_end { hw } else { 0.5 };
    let left = geom::offset_polyline(&p, hw);
    let right = geom::offset_polyline(&p, -hw);
    let mut ring = left;
    ring.extend(right.iter().rev());
    if polygon_area(&ring).abs() < 1.0 {
        return None;
    }
    Some(to_geo(&ring))
}

/// Corridor edge distance of a road: where parcels start.
pub(super) fn corridor_edge(seg: &crate::roads::Segment) -> f32 {
    if seg.is_street() { seg.half_width() + SIDEWALK } else { seg.half_width() + 1.0 }
}

/// Distance from a point to the nearest road centreline, with that
/// segment. Paths count only when `allow_paths` (homes may line a path).
pub(super) fn nearest_road(roads: &RoadNetwork, p: Vec2, allow_paths: bool) -> Option<(SegmentId, f32, f32)> {
    // Highways and bridges front nothing: no driveway joins either.
    let (sid, t, q) = roads.nearest_where(p, |seg| !seg.is_rail() && !seg.fronts_nothing() && !seg.bridge && (seg.is_street() || allow_paths))?;
    Some((sid, t, p.distance(q)))
}

/// Is this edge lying along a road's pavement line? Returns the segment and
/// the parameter of the edge midpoint on it.
pub(super) fn edge_on_road(roads: &RoadNetwork, a: Vec2, b: Vec2, allow_paths: bool) -> Option<(SegmentId, f32)> {
    let len = a.distance(b);
    if len < 1.0 {
        return None;
    }
    let mid = (a + b) * 0.5;
    let (sid, t, dist) = nearest_road(roads, mid, allow_paths)?;
    let seg = roads.segment(sid);
    let (p0, p1) = roads.segment_points(sid);
    let rd = (p1 - p0).normalize_or_zero();
    let ed = (b - a) / len;
    let parallel = rd.dot(ed).abs() > 0.8;
    let on_line = (dist - corridor_edge(seg)).abs() < 2.5;
    if parallel && on_line {
        Some((sid, t))
    } else {
        None
    }
}

/// Index of the longest edge that lies along a road, else the longest edge.
pub(super) fn frontage_edge(poly: &[Vec2], roads: &RoadNetwork, allow_paths: bool) -> (usize, Option<(SegmentId, f32)>) {
    let n = poly.len();
    let mut best_road: Option<(f32, usize, (SegmentId, f32))> = None;
    let mut best_any: (f32, usize) = (0.0, 0);
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let len = a.distance(b);
        if len > best_any.0 {
            best_any = (len, i);
        }
        if let Some(hit) = edge_on_road(roads, a, b, allow_paths) {
            if best_road.map_or(true, |(l, _, _)| len > l) {
                best_road = Some((len, i, hit));
            }
        }
    }
    match best_road {
        Some((_, i, hit)) => (i, Some(hit)),
        None => (best_any.1, None),
    }
}

/// Sharpest reflex (inward-pointing) corner of a counter-clockwise polygon
/// and its turning angle in radians. Gentle concavity along a curved road
/// is tolerated; only corners sharper than `min_turn` count.
pub(super) fn reflex_vertex(poly: &[Vec2], min_turn: f32) -> Option<(usize, f32)> {
    let n = poly.len();
    let mut best: Option<(usize, f32)> = None;
    for i in 0..n {
        let a = poly[(i + n - 1) % n];
        let b = poly[i];
        let c = poly[(i + 1) % n];
        let d1 = (b - a).normalize_or_zero();
        let d2 = (c - b).normalize_or_zero();
        let cross = d1.perp_dot(d2);
        if cross >= 0.0 {
            continue;
        }
        let turn = cross.abs().clamp(0.0, 1.0).asin();
        let turn = if d1.dot(d2) < 0.0 { std::f32::consts::PI - turn } else { turn };
        if turn > min_turn && best.map_or(true, |(_, t)| turn > t) {
            best = Some((i, turn));
        }
    }
    best
}

/// Straight runs of road frontage: consecutive road-facing edges that bend
/// by less than ~25 degrees between them (a curved road is one run, a block
/// corner separates two). Returns (edge indices, total length), longest
/// first. Cuts are placed along a run, perpendicular to the local road, so
/// straight frontages get regular lots, curves get radial ones, and corners
/// are never cut right at the corner.
pub(super) fn frontage_runs(poly: &[Vec2], roads: &RoadNetwork, allow_paths: bool) -> Vec<(Vec<usize>, f32)> {
    let n = poly.len();
    let on: Vec<bool> = (0..n).map(|i| edge_on_road(roads, poly[i], poly[(i + 1) % n], allow_paths).is_some()).collect();
    if !on.iter().any(|&b| b) {
        return Vec::new();
    }
    let dir = |i: usize| (poly[(i + 1) % n] - poly[i]).normalize_or_zero();
    // Start at an edge that begins a run: after a gap or after a corner.
    let begins = |i: usize| {
        let prev = (i + n - 1) % n;
        !on[prev] || dir(prev).dot(dir(i)) < 0.9
    };
    let start = (0..n).find(|&i| on[i] && begins(i)).unwrap_or(0);
    let mut runs: Vec<(Vec<usize>, f32)> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_len = 0.0;
    for k in 0..n {
        let i = (start + k) % n;
        if on[i] {
            if k > 0 && begins(i) && !cur.is_empty() {
                runs.push((std::mem::take(&mut cur), cur_len));
                cur_len = 0.0;
            }
            cur.push(i);
            cur_len += poly[i].distance(poly[(i + 1) % n]);
        } else if !cur.is_empty() {
            runs.push((std::mem::take(&mut cur), cur_len));
            cur_len = 0.0;
        }
    }
    if !cur.is_empty() {
        runs.push((cur, cur_len));
    }
    // A winding frontage (an S-curve is one run) is split into chunks that
    // turn by less than about 50 degrees in total, so each chunk has a
    // meaningful inward direction to peel a row from.
    let mut chunked: Vec<(Vec<usize>, f32)> = Vec::new();
    for (chain, _) in runs {
        let mut cur: Vec<usize> = Vec::new();
        let mut cur_len = 0.0;
        let mut turn = 0.0;
        for (k, &i) in chain.iter().enumerate() {
            if k > 0 {
                let d0 = dir(chain[k - 1]);
                let d1 = dir(i);
                turn += d0.perp_dot(d1).abs().clamp(0.0, 1.0).asin();
                if turn > 0.9 && cur_len > 20.0 {
                    chunked.push((std::mem::take(&mut cur), cur_len));
                    cur_len = 0.0;
                    turn = 0.0;
                }
            }
            cur.push(i);
            cur_len += poly[i].distance(poly[(i + 1) % n]);
        }
        if !cur.is_empty() {
            chunked.push((cur, cur_len));
        }
    }
    chunked.sort_by(|a, b| b.1.total_cmp(&a.1));
    chunked
}

/// Does a parcel have a usable road edge?
pub(super) fn fronted(poly: &[Vec2], lu: LandUse, roads: &RoadNetwork) -> bool {
    let (fi, road) = frontage_edge(poly, roads, lu.fronts_paths());
    let n = poly.len();
    road.is_some() && poly[fi].distance(poly[(fi + 1) % n]) >= MIN_FRONTAGE.min(lu.lot_width() * 0.45)
}

/// [free-placement] Does an outline front a street or footpath at all,
/// whatever its shape? A city building's recut lot keeps the building as
/// long as it does (a sliver or a sharp corner makes ground unfit to grow
/// on, not a placed plant or station any less reachable).
pub fn fronts_a_way(poly: &[Vec2], roads: &RoadNetwork) -> bool {
    poly.len() >= 3 && frontage_edge(poly, roads, true).1.is_some()
}

/// Ground within `r` of a road's centreline, as overlapping convex pieces
/// (a rectangle per leg, a disc at every bend) for one robust union. The
/// ends stop where the corridor does, so a dead end grows no backland cap.
pub(super) fn reach_pieces(pts: &[Vec2], hw: f32, r: f32, ext_start: bool, ext_end: bool, out: &mut Vec<Polygon<f64>>) {
    if pts.len() < 2 {
        return;
    }
    let mut p = pts.to_vec();
    let n = p.len();
    let d0 = (p[1] - p[0]).normalize_or_zero();
    let d1 = (p[n - 1] - p[n - 2]).normalize_or_zero();
    p[0] -= d0 * if ext_start { hw } else { 0.5 };
    p[n - 1] += d1 * if ext_end { hw } else { 0.5 };
    for w in p.windows(2) {
        let d = (w[1] - w[0]).normalize_or_zero();
        if d == Vec2::ZERO {
            continue;
        }
        let nrm = Vec2::new(-d.y, d.x) * r;
        out.push(to_geo(&[w[0] - nrm, w[1] - nrm, w[1] + nrm, w[0] + nrm]));
    }
    for &c in &p[1..n - 1] {
        let ring: Vec<Vec2> = (0..16).map(|k| {
            let a = k as f32 / 16.0 * std::f32::consts::TAU;
            c + Vec2::new(a.cos(), a.sin()) * r
        }).collect();
        out.push(to_geo(&ring));
    }
}

/// Width along the frontage `lu` would use, or None with no frontage.
pub(super) fn frontage_width(poly: &[Vec2], lu: LandUse, roads: &RoadNetwork) -> Option<f32> {
    let (fi, road) = frontage_edge(poly, roads, lu.fronts_paths());
    road?;
    let n = poly.len();
    let along = (poly[(fi + 1) % n] - poly[fi]).normalize_or_zero();
    if along == Vec2::ZERO {
        return None;
    }
    let (u0, u1) = extents(poly, along);
    Some(u1 - u0)
}
