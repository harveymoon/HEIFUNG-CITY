//! Streamlines to a street network: dangling ends joined to the street
//! ahead, crossings made into junctions, then cleaned to the game's rules
//! (no short pieces, no junction angles too sharp to turn, no sliver
//! blocks, one connected network), simplified, and read back as blocks
//! (the faces between the streets) and ways (the streets as drawn).

use super::trace::{Ground, Level, Line, Site, STEP};
use glam::Vec2;
use nd_sim::geom::{path_length, polygon_area, project_on_segment};

/// Junctions closer than this are one junction.
const MERGE_RADIUS: f32 = 10.0;
/// Shortest street between two junctions.
pub const MIN_EDGE: f32 = 22.0;
/// Shortest dead end kept.
const MIN_DANGLE: f32 = 45.0;
/// Sharpest angle between two streets at a junction.
pub const MIN_ANGLE_DEG: f32 = 32.0;
/// Smallest block, square metres, and the thinnest (area over perimeter²).
const MIN_BLOCK: f32 = 1800.0;
const MIN_COMPACTNESS: f32 = 0.022;
/// Simplification tolerance, metres.
const SIMPLIFY: f32 = 0.9;

#[derive(Clone, Debug)]
pub struct Node {
    pub pos: Vec2,
    pub edges: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub a: usize,
    pub b: usize,
    /// From node a to node b, both included.
    pub pts: Vec<Vec2>,
    pub level: Level,
    /// The streamline it came from.
    pub line: usize,
    pub alive: bool,
}

impl Edge {
    pub fn length(&self) -> f32 {
        path_length(&self.pts)
    }

    pub fn other(&self, n: usize) -> usize {
        if self.a == n { self.b } else { self.a }
    }

    /// The edge's points leaving node `n`.
    pub fn from(&self, n: usize) -> Vec<Vec2> {
        if self.a == n { self.pts.clone() } else { self.pts.iter().rev().copied().collect() }
    }
}

/// Direction of a polyline leaving its first point, read `reach` along.
fn heading(pts: &[Vec2], reach: f32) -> Vec2 {
    let p0 = pts[0];
    let mut s = 0.0;
    for w in pts.windows(2) {
        s += w[0].distance(w[1]);
        if s >= reach {
            return (w[1] - p0).normalize_or_zero();
        }
    }
    (pts[pts.len() - 1] - p0).normalize_or_zero()
}

/// Douglas-Peucker.
pub fn simplify(pts: &[Vec2], tol: f32) -> Vec<Vec2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((i, j)) = stack.pop() {
        let mut best = (0.0f32, i);
        for k in i + 1..j {
            let d = project_on_segment(pts[k], pts[i], pts[j]).1.distance(pts[k]);
            if d > best.0 {
                best = (d, k);
            }
        }
        if best.0 > tol {
            keep[best.1] = true;
            stack.push((i, best.1));
            stack.push((best.1, j));
        }
    }
    pts.iter().zip(keep).filter(|(_, k)| *k).map(|(p, _)| *p).collect()
}

fn seg_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<(f32, f32)> {
    let r = b - a;
    let s = d - c;
    let den = r.perp_dot(s);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = (c - a).perp_dot(s) / den;
    let u = (c - a).perp_dot(r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

/// Segments of many polylines, bucketed.
struct SegGrid {
    origin: Vec2,
    cell: f32,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<(u32, u32)>>,
}

impl SegGrid {
    fn new(lines: &[Vec<Vec2>], cell: f32) -> Self {
        let mut lo = Vec2::splat(f32::INFINITY);
        let mut hi = Vec2::splat(f32::NEG_INFINITY);
        for l in lines {
            for p in l {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        if !lo.is_finite() {
            lo = Vec2::ZERO;
            hi = Vec2::ONE;
        }
        lo -= Vec2::splat(cell);
        let nx = ((hi.x - lo.x) / cell).ceil() as usize + 2;
        let ny = ((hi.y - lo.y) / cell).ceil() as usize + 2;
        let mut g = Self { origin: lo, cell, nx, ny, cells: vec![Vec::new(); nx * ny] };
        for (li, l) in lines.iter().enumerate() {
            for (si, w) in l.windows(2).enumerate() {
                let (i0, j0, i1, j1) = g.range(w[0].min(w[1]), w[0].max(w[1]));
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        g.cells[j * nx + i].push((li as u32, si as u32));
                    }
                }
            }
        }
        g
    }

    fn range(&self, lo: Vec2, hi: Vec2) -> (usize, usize, usize, usize) {
        let c = |v: f32, o: f32, n: usize| (((v - o) / self.cell).floor().max(0.0) as usize).min(n - 1);
        (c(lo.x, self.origin.x, self.nx), c(lo.y, self.origin.y, self.ny), c(hi.x, self.origin.x, self.nx), c(hi.y, self.origin.y, self.ny))
    }

    /// Candidate segments near a box, sorted and unique.
    fn query(&self, lo: Vec2, hi: Vec2) -> Vec<(u32, u32)> {
        let (i0, j0, i1, j1) = self.range(lo, hi);
        let mut out = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                out.extend_from_slice(&self.cells[j * self.nx + i]);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }
}

/// Carry each dangling street end on to the street ahead of it (within
/// `lookahead` for its level), or onto a street just beside it.
pub fn join_ends(lines: &mut [Line], site: &Site, lookahead: [f32; 3]) {
    let polys: Vec<Vec<Vec2>> = lines.iter().map(|l| l.pts.clone()).collect();
    let grid = SegGrid::new(&polys, 40.0);
    for li in 0..lines.len() {
        if lines[li].closed || lines[li].pts.len() < 4 {
            continue;
        }
        let level = lines[li].level;
        let reach = lookahead[level as usize];
        for end in [false, true] {
            let pts = &lines[li].pts;
            let n = pts.len();
            let (p, back) = if end { (pts[n - 1], pts[n.saturating_sub(4)]) } else { (pts[0], pts[3.min(n - 1)]) };
            let d = (p - back).normalize_or_zero();
            if d == Vec2::ZERO {
                continue;
            }
            let q = p + d * reach;
            let mut best: Option<(f32, Vec2, Vec2)> = None;
            for (lj, sj) in grid.query(p.min(q), p.max(q)) {
                let (lj, sj) = (lj as usize, sj as usize);
                // Not the street's own last stretch.
                if lj == li && (if end { sj + 8 >= n } else { sj <= 8 }) {
                    continue;
                }
                let (a, b) = (polys[lj][sj], polys[lj][sj + 1]);
                if let Some((t, _)) = seg_cross(p, q, a, b) {
                    let dist = t * reach;
                    if dist > 0.5 && best.is_none_or(|(bd, ..)| dist < bd) {
                        best = Some((dist, p + d * dist, (b - a).normalize_or_zero()));
                    }
                }
            }
            let Some((dist, hit, along)) = best else { continue };
            // Meets it at a turnable angle, over buildable ground.
            if d.perp_dot(along).abs() < (MIN_ANGLE_DEG + 8.0).to_radians().sin() {
                continue;
            }
            let steps = (dist / STEP).ceil() as usize;
            let ok = (1..steps).all(|k| {
                let g = site.ground(p + d * (k as f32 * STEP));
                g == Ground::Ok || (g == Ground::Rail && level <= site.rail_cross)
            });
            if !ok {
                continue;
            }
            // A little past, so the crossing is found exactly.
            let tip = hit + d * 1.0;
            let pts = &mut lines[li].pts;
            if end {
                pts.push(tip);
            } else {
                pts.insert(0, tip);
            }
        }
    }
}

/// Run street ends that stop at a plaza's clearing, pointing at it, in to
/// its centre.
pub fn join_plazas(lines: &mut [Line], plazas: &[Vec2]) {
    for line in lines.iter_mut() {
        if line.closed || line.pts.len() < 4 {
            continue;
        }
        for end in [false, true] {
            let n = line.pts.len();
            let (p, back) = if end { (line.pts[n - 1], line.pts[n - 4]) } else { (line.pts[0], line.pts[3]) };
            let d = (p - back).normalize_or_zero();
            for &c in plazas {
                let to = c - p;
                if to.length() < super::trace::PLAZA_CLEAR + 30.0 && d.dot(to.normalize_or_zero()) > 0.8 {
                    if end {
                        line.pts.push(c);
                    } else {
                        line.pts.insert(0, c);
                    }
                    break;
                }
            }
        }
    }
}

/// The street network.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Position `u` (segment index plus fraction) along a polyline.
fn at_param(pts: &[Vec2], u: f32) -> Vec2 {
    let i = (u.floor() as usize).min(pts.len() - 2);
    pts[i].lerp(pts[i + 1], u - i as f32)
}

/// The stretch of a polyline from `u0` to `u1` (u0 < u1).
fn sub_line(pts: &[Vec2], u0: f32, u1: f32) -> Vec<Vec2> {
    let mut out = vec![at_param(pts, u0)];
    let i0 = u0.floor() as usize + 1;
    let i1 = u1.ceil() as usize;
    for i in i0..i1.min(pts.len()) {
        if (i as f32) > u0 && (i as f32) < u1 {
            out.push(pts[i]);
        }
    }
    out.push(at_param(pts, u1));
    out
}

/// Set a polyline's end to `p`, dropping points that would fold back
/// within a few metres of it.
fn reanchor(mut pts: Vec<Vec2>, start: Option<Vec2>, end: Option<Vec2>) -> Vec<Vec2> {
    if let Some(p) = start {
        while pts.len() > 2 && pts[1].distance(p) < 5.0 {
            pts.remove(1);
        }
        pts[0] = p;
    }
    if let Some(p) = end {
        while pts.len() > 2 && pts[pts.len() - 2].distance(p) < 5.0 {
            let n = pts.len();
            pts.remove(n - 2);
        }
        let n = pts.len();
        pts[n - 1] = p;
    }
    pts
}

impl Graph {
    /// Junctions wherever streets cross, edges between them.
    pub fn build(lines: &[Line]) -> Graph {
        let polys: Vec<Vec<Vec2>> = lines.iter().map(|l| l.pts.clone()).collect();
        let grid = SegGrid::new(&polys, 30.0);
        // Crossings: (point, line a, param a, line b, param b).
        let mut hits: Vec<(Vec2, usize, f32, usize, f32)> = Vec::new();
        for (la, pa) in polys.iter().enumerate() {
            for (sa, w) in pa.windows(2).enumerate() {
                for (lb, sb) in grid.query(w[0].min(w[1]), w[0].max(w[1])) {
                    let (lb, sb) = (lb as usize, sb as usize);
                    if (lb, sb) <= (la, sa) {
                        continue;
                    }
                    if lb == la {
                        let n = pa.len() - 1;
                        let gap = sb - sa;
                        if gap <= 1 || (lines[la].closed && gap == n - 1) {
                            continue;
                        }
                    }
                    let (c, d) = (polys[lb][sb], polys[lb][sb + 1]);
                    if let Some((t, u)) = seg_cross(w[0], w[1], c, d) {
                        hits.push((w[0].lerp(w[1], t), la, sa as f32 + t, lb, sb as f32 + u));
                    }
                }
            }
        }
        // Cluster crossings closer than MERGE_RADIUS (union-find).
        let mut parent: Vec<usize> = (0..hits.len()).collect();
        fn find(p: &mut [usize], i: usize) -> usize {
            let mut r = i;
            while p[r] != r {
                r = p[r];
            }
            let mut i = i;
            while p[i] != r {
                let n = p[i];
                p[i] = r;
                i = n;
            }
            r
        }
        {
            let pts: Vec<Vec<Vec2>> = hits.iter().map(|h| vec![h.0, h.0]).collect();
            let hg = SegGrid::new(&pts, MERGE_RADIUS * 2.0);
            for i in 0..hits.len() {
                let p = hits[i].0;
                for (j, _) in hg.query(p - Vec2::splat(MERGE_RADIUS), p + Vec2::splat(MERGE_RADIUS)) {
                    let j = j as usize;
                    if j > i && hits[j].0.distance(p) < MERGE_RADIUS {
                        let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                        if ri != rj {
                            parent[ri.max(rj)] = ri.min(rj);
                        }
                    }
                }
            }
        }
        let mut g = Graph::default();
        let mut node_of_root: Vec<Option<usize>> = vec![None; hits.len()];
        let mut sums: Vec<(Vec2, f32)> = Vec::new();
        let mut hit_node = vec![0usize; hits.len()];
        for i in 0..hits.len() {
            let r = find(&mut parent, i);
            let n = *node_of_root[r].get_or_insert_with(|| {
                sums.push((Vec2::ZERO, 0.0));
                sums.len() - 1
            });
            // Weighted towards the bigger streets' line.
            let w = 1.0 / (1.0 + lines[hits[i].1].level.min(lines[hits[i].3].level) as usize as f32 * 2.0);
            sums[n].0 += hits[i].0 * w;
            sums[n].1 += w;
            hit_node[i] = n;
        }
        g.nodes = sums.iter().map(|(s, w)| Node { pos: *s / *w, edges: Vec::new() }).collect();
        // Each line's junctions in order along it.
        let mut events: Vec<Vec<(f32, usize)>> = vec![Vec::new(); lines.len()];
        for (i, h) in hits.iter().enumerate() {
            events[h.1].push((h.2, hit_node[i]));
            events[h.3].push((h.4, hit_node[i]));
        }
        for (li, line) in lines.iter().enumerate() {
            let pts = &line.pts;
            let last = (pts.len() - 1) as f32;
            let mut ev = std::mem::take(&mut events[li]);
            ev.sort_by(|a, b| a.0.total_cmp(&b.0));
            ev.dedup_by(|b, a| a.1 == b.1);
            if ev.is_empty() {
                continue;
            }
            let mut stops: Vec<(f32, usize)> = Vec::new();
            if line.closed {
                stops.extend(ev.iter().copied());
                // Round the loop back to the first junction.
                let first = ev[0];
                stops.push((first.0 + last, first.1));
            } else {
                if ev[0].0 > 0.05 {
                    g.nodes.push(Node { pos: pts[0], edges: Vec::new() });
                    stops.push((0.0, g.nodes.len() - 1));
                }
                stops.extend(ev.iter().copied());
                if ev[ev.len() - 1].0 < last - 0.05 {
                    g.nodes.push(Node { pos: pts[pts.len() - 1], edges: Vec::new() });
                    stops.push((last, g.nodes.len() - 1));
                }
            }
            for w in stops.windows(2) {
                let ((u0, a), (u1, b)) = (w[0], w[1]);
                if a == b {
                    continue;
                }
                let raw = if u1 <= last {
                    sub_line(pts, u0, u1)
                } else {
                    let mut s = sub_line(pts, u0, last);
                    s.extend(sub_line(pts, 0.0, u1 - last).into_iter().skip(1));
                    s
                };
                let pts = reanchor(raw, Some(g.nodes[a].pos), Some(g.nodes[b].pos));
                g.add_edge(a, b, pts, line.level, li);
            }
        }
        g
    }

    fn add_edge(&mut self, a: usize, b: usize, pts: Vec<Vec2>, level: Level, line: usize) -> usize {
        let id = self.edges.len();
        self.edges.push(Edge { a, b, pts, level, line, alive: true });
        self.nodes[a].edges.push(id);
        if b != a {
            self.nodes[b].edges.push(id);
        }
        id
    }

    fn kill(&mut self, e: usize) {
        if !self.edges[e].alive {
            return;
        }
        self.edges[e].alive = false;
        let (a, b) = (self.edges[e].a, self.edges[e].b);
        self.nodes[a].edges.retain(|&x| x != e);
        self.nodes[b].edges.retain(|&x| x != e);
    }

    pub fn degree(&self, n: usize) -> usize {
        self.nodes[n].edges.len()
    }

    pub fn alive_edges(&self) -> impl Iterator<Item = (usize, &Edge)> {
        self.edges.iter().enumerate().filter(|(_, e)| e.alive)
    }

    /// Short dead ends, and dead ends that stop just short of another
    /// street (the game would snap them onto it).
    fn prune_dangles(&mut self) -> bool {
        let mut changed = false;
        for e in 0..self.edges.len() {
            if !self.edges[e].alive {
                continue;
            }
            let (a, b) = (self.edges[e].a, self.edges[e].b);
            let dead = [a, b].into_iter().filter(|&n| self.degree(n) == 1).collect::<Vec<_>>();
            if dead.is_empty() {
                continue;
            }
            let len = self.edges[e].length();
            if len < MIN_DANGLE || dead.len() == 2 && len < 150.0 {
                self.kill(e);
                changed = true;
                continue;
            }
            for n in dead {
                let p = self.nodes[n].pos;
                if self.near_other_edge(p, e, 15.0) {
                    // Pull the end back clear.
                    let mut pts = self.edges[e].from(n);
                    let mut cut = 0.0;
                    while pts.len() > 2 && cut < 20.0 {
                        cut += pts[0].distance(pts[1]);
                        pts.remove(0);
                    }
                    if path_length(&pts) < MIN_DANGLE || pts.len() < 2 {
                        self.kill(e);
                    } else {
                        self.nodes[n].pos = pts[0];
                        let ed = &mut self.edges[e];
                        ed.pts = if ed.a == n { pts } else { pts.into_iter().rev().collect() };
                    }
                    changed = true;
                    break;
                }
            }
        }
        changed
    }

    fn near_other_edge(&self, p: Vec2, me: usize, r: f32) -> bool {
        self.alive_edges().any(|(i, e)| {
            if i == me {
                return false;
            }
            e.pts.windows(2).any(|w| project_on_segment(p, w[0], w[1]).1.distance(p) < r)
        })
    }

    /// Merge the ends of edges shorter than `MIN_EDGE`.
    fn collapse_short(&mut self) -> bool {
        let mut changed = false;
        for e in 0..self.edges.len() {
            if !self.edges[e].alive || self.edges[e].length() >= MIN_EDGE {
                continue;
            }
            let (a, b) = (self.edges[e].a, self.edges[e].b);
            if a == b {
                self.kill(e);
                changed = true;
                continue;
            }
            if self.degree(a) == 1 || self.degree(b) == 1 {
                self.kill(e);
                changed = true;
                continue;
            }
            // Keep the junction on the bigger street.
            let rank = |g: &Graph, n: usize| g.nodes[n].edges.iter().map(|&x| 3 - g.edges[x].level as usize).sum::<usize>();
            let (keep, gone) = if rank(self, a) >= rank(self, b) { (a, b) } else { (b, a) };
            let pos = self.nodes[keep].pos;
            self.kill(e);
            let moved: Vec<usize> = self.nodes[gone].edges.clone();
            for x in moved {
                let ed = &mut self.edges[x];
                if ed.a == gone {
                    ed.a = keep;
                    ed.pts = reanchor(std::mem::take(&mut ed.pts), Some(pos), None);
                }
                if ed.b == gone {
                    ed.b = keep;
                    ed.pts = reanchor(std::mem::take(&mut ed.pts), None, Some(pos));
                }
                self.nodes[keep].edges.push(x);
            }
            self.nodes[gone].edges.clear();
            changed = true;
        }
        changed
    }

    /// Drop the lesser of two streets meeting at too sharp an angle, and
    /// one of two edges joining the same junctions.
    fn fix_angles(&mut self) -> bool {
        let mut changed = false;
        let min = MIN_ANGLE_DEG.to_radians();
        for n in 0..self.nodes.len() {
            let es = self.nodes[n].edges.clone();
            if es.len() < 2 {
                continue;
            }
            // Read near and further along: the game draws the first piece
            // straight to wherever the street has got to by then.
            let reaches = [8.0, 14.0, 22.0];
            let dirs: Vec<(usize, Vec<Vec2>)> = es.iter().map(|&e| (e, reaches.iter().map(|&r| heading(&self.edges[e].from(n), r)).collect())).collect();
            let mut victim = None;
            'pairs: for i in 0..dirs.len() {
                for j in i + 1..dirs.len() {
                    let d = (0..reaches.len()).map(|k| dirs[i].1[k].dot(dirs[j].1[k]).clamp(-1.0, 1.0).acos()).fold(f32::INFINITY, f32::min);
                    let (ei, ej) = (dirs[i].0, dirs[j].0);
                    let same_ends = { let (x, y) = (&self.edges[ei], &self.edges[ej]); (x.a == y.a && x.b == y.b) || (x.a == y.b && x.b == y.a) };
                    if d < min || same_ends {
                        let (x, y) = (&self.edges[ei], &self.edges[ej]);
                        victim = Some(if (x.level, -x.length()) > (y.level, -y.length()) { ei } else { ej });
                        break 'pairs;
                    }
                }
            }
            if let Some(v) = victim {
                self.kill(v);
                changed = true;
            }
        }
        changed
    }

    /// Join the two edges at a junction left with only them, when they are
    /// the same kind of street.
    fn merge_through(&mut self) -> bool {
        let mut changed = false;
        for n in 0..self.nodes.len() {
            if self.degree(n) != 2 {
                continue;
            }
            let (e1, e2) = (self.nodes[n].edges[0], self.nodes[n].edges[1]);
            if e1 == e2 || self.edges[e1].level != self.edges[e2].level {
                continue;
            }
            let (x, y) = (self.edges[e1].other(n), self.edges[e2].other(n));
            if x == n || y == n || x == y {
                continue;
            }
            let mut pts: Vec<Vec2> = self.edges[e1].from(n).into_iter().rev().collect();
            pts.extend(self.edges[e2].from(n).into_iter().skip(1));
            let (level, line) = (self.edges[e1].level, self.edges[e1].line);
            self.kill(e1);
            self.kill(e2);
            self.add_edge(x, y, pts, level, line);
            changed = true;
        }
        changed
    }

    /// At most `max` streets into the junction at `c` (a plaza's centre),
    /// well spread round it: the arterials first, then the longest. Every
    /// street into a centre is a junction full of turning traffic.
    pub fn thin_star(&mut self, c: Vec2, max: usize) {
        let Some(n) = (0..self.nodes.len()).find(|&n| self.nodes[n].pos.distance(c) < 1.0 && !self.nodes[n].edges.is_empty()) else { return };
        let mut es = self.nodes[n].edges.clone();
        es.sort_by(|&a, &b| self.edges[a].level.cmp(&self.edges[b].level).then(self.edges[b].length().total_cmp(&self.edges[a].length())).then(a.cmp(&b)));
        let mut kept: Vec<Vec2> = Vec::new();
        for e in es {
            let h = heading(&self.edges[e].from(n), 40.0);
            if kept.len() < max && kept.iter().all(|k| k.dot(h) < 0.5) {
                kept.push(h);
            } else {
                self.kill(e);
            }
        }
    }

    /// Keep only the biggest connected network (by length of street).
    fn keep_largest(&mut self) {
        let mut comp = vec![usize::MAX; self.nodes.len()];
        let mut sizes: Vec<f32> = Vec::new();
        for s in 0..self.nodes.len() {
            if comp[s] != usize::MAX || self.nodes[s].edges.is_empty() {
                continue;
            }
            let c = sizes.len();
            sizes.push(0.0);
            let mut stack = vec![s];
            comp[s] = c;
            while let Some(n) = stack.pop() {
                for &e in &self.nodes[n].edges {
                    let o = self.edges[e].other(n);
                    if comp[o] == usize::MAX {
                        comp[o] = c;
                        stack.push(o);
                    }
                }
            }
        }
        for (_, e) in self.alive_edges() {
            sizes[comp[e.a]] += e.length();
        }
        let Some(best) = (0..sizes.len()).max_by(|&a, &b| sizes[a].total_cmp(&sizes[b])) else { return };
        for e in 0..self.edges.len() {
            if self.edges[e].alive && comp[self.edges[e].a] != best {
                self.kill(e);
            }
        }
    }

    /// Faces: each a closed polygon (counter-clockwise for blocks,
    /// clockwise for the outside) and the half-edges round it, as
    /// (edge, forward).
    pub fn faces(&self) -> Vec<(Vec<Vec2>, Vec<(usize, bool)>)> {
        // Outgoing half-edges at each node, sorted by angle.
        let mut out: Vec<Vec<(f32, usize, bool)>> = vec![Vec::new(); self.nodes.len()];
        for (i, e) in self.alive_edges() {
            let h = heading(&e.pts, 8.0);
            out[e.a].push((h.y.atan2(h.x), i, true));
            let r: Vec<Vec2> = e.pts.iter().rev().copied().collect();
            let h = heading(&r, 8.0);
            out[e.b].push((h.y.atan2(h.x), i, false));
        }
        for o in &mut out {
            o.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        }
        let mut seen = std::collections::HashSet::new();
        let mut faces = Vec::new();
        for (i, _) in self.alive_edges() {
            for fwd in [true, false] {
                if seen.contains(&(i, fwd)) {
                    continue;
                }
                let mut ring = Vec::new();
                let mut halves = Vec::new();
                let (mut e, mut f) = (i, fwd);
                for _ in 0..100_000 {
                    if !seen.insert((e, f)) {
                        break;
                    }
                    halves.push((e, f));
                    let ed = &self.edges[e];
                    let pts: Vec<Vec2> = if f { ed.pts.clone() } else { ed.pts.iter().rev().copied().collect() };
                    ring.extend_from_slice(&pts[..pts.len() - 1]);
                    let v = if f { ed.b } else { ed.a };
                    let list = &out[v];
                    let k = list.iter().position(|&(_, x, xf)| x == e && xf == !f).unwrap_or(0);
                    let prev = list[(k + list.len() - 1) % list.len()];
                    e = prev.1;
                    f = prev.2;
                }
                faces.push((remove_spikes(ring), halves));
            }
        }
        faces
    }

    /// Remove the lesser street round each sliver block.
    fn remove_slivers(&mut self) -> bool {
        let mut victims = Vec::new();
        for (ring, halves) in self.faces() {
            let area = polygon_area(&ring);
            if area <= 0.0 {
                continue;
            }
            let per = nd_sim::geom::perimeter(&ring);
            if area >= MIN_BLOCK && area / (per * per) >= MIN_COMPACTNESS {
                continue;
            }
            let v = halves.iter().map(|h| h.0).max_by(|&a, &b| {
                let (x, y) = (&self.edges[a], &self.edges[b]);
                x.level.cmp(&y.level).then(y.length().total_cmp(&x.length())).then(a.cmp(&b))
            });
            if let Some(v) = v {
                victims.push(v);
            }
        }
        victims.sort_unstable();
        victims.dedup();
        let changed = !victims.is_empty();
        for v in victims {
            self.kill(v);
        }
        changed
    }

    /// Clean the network to the game's rules.
    pub fn clean(&mut self) {
        for _ in 0..12 {
            let mut changed = false;
            for _ in 0..20 {
                let c = self.prune_dangles() | self.collapse_short() | self.fix_angles() | self.merge_through();
                changed |= c;
                if !c {
                    break;
                }
            }
            self.keep_largest();
            if !self.remove_slivers() && !changed {
                break;
            }
        }
        self.keep_largest();
        for e in &mut self.edges {
            if e.alive {
                e.pts = simplify(&e.pts, SIMPLIFY);
            }
        }
    }

    /// Streets as drawn: edges chained through junctions where the same
    /// streamline carries on, each with its level.
    pub fn ways(&self) -> Vec<(Vec<Vec2>, Level)> {
        let mut used = vec![false; self.edges.len()];
        let mut out = Vec::new();
        // Carry on through node `n` from edge `e`: the unused edge of the
        // same line and level that runs on straightest.
        let next = |used: &[bool], e: usize, n: usize| -> Option<usize> {
            let me = &self.edges[e];
            let into = -heading(&me.from(n), 15.0);
            self.nodes[n]
                .edges
                .iter()
                .copied()
                .filter(|&x| x != e && !used[x] && self.edges[x].line == me.line && self.edges[x].level == me.level)
                .map(|x| (x, heading(&self.edges[x].from(n), 15.0).dot(into)))
                .filter(|(_, c)| *c > 0.5)
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(x, _)| x)
        };
        for (i, e) in self.alive_edges() {
            if used[i] {
                continue;
            }
            used[i] = true;
            let mut pts = e.pts.clone();
            // Forwards from b.
            let (mut cur, mut n) = (i, e.b);
            while let Some(x) = next(&used, cur, n) {
                used[x] = true;
                pts.extend(self.edges[x].from(n).into_iter().skip(1));
                n = self.edges[x].other(n);
                cur = x;
                if n == e.a {
                    break;
                }
            }
            // Backwards from a.
            let (mut cur, mut n) = (i, e.a);
            while let Some(x) = next(&used, cur, n) {
                used[x] = true;
                let mut head: Vec<Vec2> = self.edges[x].from(n).into_iter().rev().collect();
                head.pop();
                head.extend(pts);
                pts = head;
                n = self.edges[x].other(n);
                cur = x;
            }
            out.push((pts, e.level));
        }
        out
    }
}

/// Drop back-and-forth spurs from a face ring (a dead end inside a block
/// is walked out and back).
pub fn remove_spikes(ring: Vec<Vec2>) -> Vec<Vec2> {
    let mut st: Vec<Vec2> = Vec::with_capacity(ring.len());
    for p in ring {
        if st.last().is_some_and(|q| q.distance(p) < 0.01) {
            continue;
        }
        if st.len() >= 2 && st[st.len() - 2].distance(p) < 0.5 {
            st.pop();
            continue;
        }
        st.push(p);
    }
    // And across the seam.
    loop {
        let n = st.len();
        if n < 4 {
            break;
        }
        if st[n - 1].distance(st[0]) < 0.01 {
            st.pop();
        } else if st[n - 1].distance(st[1]) < 0.5 {
            st.remove(0);
            st.pop();
        } else if st[n - 2].distance(st[0]) < 0.5 {
            st.pop();
            st.pop();
        } else {
            break;
        }
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citygen::trace::Family;

    fn line(a: Vec2, b: Vec2, level: Level) -> Line {
        let n = (a.distance(b) / STEP) as usize;
        Line { pts: (0..=n).map(|k| a.lerp(b, k as f32 / n as f32)).collect(), level, family: Family::Major, closed: false }
    }

    #[test]
    fn a_grid_becomes_junctions_blocks_and_ways() {
        // Three streets each way, 100 m apart, overrunning by 10 m: the
        // stubs past the outer streets go, the crossings become junctions.
        let mut lines = Vec::new();
        for k in 0..3 {
            let c = k as f32 * 100.0;
            lines.push(line(Vec2::new(-10.0, c), Vec2::new(210.0, c), Level::Main));
            lines.push(line(Vec2::new(c, -10.0), Vec2::new(c, 210.0), Level::Minor));
        }
        let mut g = Graph::build(&lines);
        g.clean();
        let junctions = g.nodes.iter().filter(|n| !n.edges.is_empty()).count();
        assert_eq!(junctions, 9);
        let faces = g.faces();
        let blocks: Vec<f32> = faces.iter().map(|(r, _)| polygon_area(r)).filter(|a| *a > 0.0).collect();
        assert_eq!(blocks.len(), 4, "{blocks:?}");
        assert!(blocks.iter().all(|a| (a - 10_000.0).abs() < 1.0), "{blocks:?}");
        assert_eq!(faces.iter().filter(|(r, _)| polygon_area(r) < 0.0).count(), 1, "one outside");
        // Each street is one way, junction to junction.
        let ways = g.ways();
        assert_eq!(ways.len(), 6);
        assert!(ways.iter().all(|(p, _)| (path_length(p) - 200.0).abs() < 0.5));
    }

    #[test]
    fn a_sliver_block_loses_its_lesser_street() {
        // A local street 12 m from an arterial makes a sliver; it goes.
        let lines = vec![
            line(Vec2::new(0.0, -10.0), Vec2::new(0.0, 210.0), Level::Main),
            line(Vec2::new(200.0, -10.0), Vec2::new(200.0, 210.0), Level::Main),
            line(Vec2::new(-10.0, 0.0), Vec2::new(210.0, 0.0), Level::Main),
            line(Vec2::new(-10.0, 200.0), Vec2::new(210.0, 200.0), Level::Main),
            line(Vec2::new(-10.0, 188.0), Vec2::new(210.0, 188.0), Level::Minor),
        ];
        let mut g = Graph::build(&lines);
        g.clean();
        assert!(g.alive_edges().all(|(_, e)| e.level == Level::Main));
    }

    #[test]
    fn spikes_come_out_of_a_ring() {
        let ring = vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 5.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        assert_eq!(remove_spikes(ring), vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)]);
    }
}
