//! Ways over water: ferry and shipping routes on the water graph, and the
//! sea way between two points.

use super::*;

impl Landscape {
    /// A route by water from `from` to "elsewhere" (off the map), or to
    /// `to`, as a polyline. Rivers are followed along their centrelines,
    /// tributaries join the river they flow into, river mouths open onto the
    /// sea, and the sea is crossed in straight lines. `None` when the two
    /// are not on connected water (a lake, or different rivers that never
    /// meet) or `from` is not on or by the water.
    pub fn water_route(&self, from: Vec2, to: Option<Vec2>) -> Option<Vec<Vec2>> {
        let g = WaterGraph::build(self);
        g.route(self, from, to)
    }

    /// [ferries] Every edge of the map that water from `from` reaches,
    /// with the shortest route out across it, nearest first. A dock on a
    /// river that runs off both sides of the map reaches both; one on a
    /// coast reaches only the edge its sea opens onto (each coast of a
    /// two-coast map is its own way out); one on a lake reaches none. The
    /// routes end out of sight, like `water_route`'s.
    pub fn water_routes_out(&self, from: Vec2) -> Vec<(MapEdge, Vec<Vec2>)> {
        let g = WaterGraph::build(self);
        g.routes_out(self, from)
    }

    /// [ferry-routes] Many routes at once on one water graph (built once):
    /// `water_route` for each pair and `water_routes_out` for each point.
    pub fn water_routes_batch(&self, pairs: &[(Vec2, Option<Vec2>)], outs: &[Vec2]) -> (Vec<Option<Vec<Vec2>>>, Vec<Vec<(MapEdge, Vec<Vec2>)>>) {
        if pairs.is_empty() && outs.is_empty() {
            return (Vec::new(), Vec::new());
        }
        let g = WaterGraph::build(self);
        (pairs.iter().map(|(a, b)| g.route(self, *a, *b)).collect(), outs.iter().map(|p| g.routes_out(self, *p)).collect())
    }

    /// [ferry-routes] Is the straight line from `a` to `b` on water (or
    /// off the map) the whole way?
    pub(super) fn clear_water(&self, a: Vec2, b: Vec2) -> bool {
        let n = (a.distance(b) / 4.0).ceil().max(1.0) as usize;
        (0..=n).all(|i| {
            let p = a.lerp(b, i as f32 / n as f32);
            chebyshev(p) > LAND_HALF || self.is_water(p)
        })
    }

    /// [ferry-routes] A way over open water from `a` to `b` that keeps off
    /// the land: straight when nothing is in the way, else round the
    /// islands and headlands on a grid of water cells, pulled taut. Ships
    /// used to sail straight across the sea, islands and all.
    pub fn sea_way(&self, a: Vec2, b: Vec2) -> Vec<Vec2> {
        if self.clear_water(a, b) {
            return vec![a, b];
        }
        const CELL: f32 = 30.0;
        let half = LAND_HALF + 2.0 * CELL;
        let n = (2.0 * half / CELL).ceil() as i32;
        let centre = |i: i32, j: i32| Vec2::new(-half + (i as f32 + 0.5) * CELL, -half + (j as f32 + 0.5) * CELL);
        let water = |p: Vec2| chebyshev(p) > LAND_HALF || self.is_water(p);
        // A cell is open water when its centre and all four corners are, so
        // a hop between neighbours never clips a point of land.
        let corner: Vec<bool> = (0..(n + 1) * (n + 1)).map(|k| water(Vec2::new(-half + (k % (n + 1)) as f32 * CELL, -half + (k / (n + 1)) as f32 * CELL))).collect();
        let wet: Vec<bool> = (0..n * n)
            .map(|k| {
                let (i, j) = (k % n, k / n);
                let c = |di: i32, dj: i32| corner[((i + di) + (j + dj) * (n + 1)) as usize];
                c(0, 0) && c(1, 0) && c(0, 1) && c(1, 1) && water(centre(i, j))
            })
            .collect();
        let cell_of = |p: Vec2| (((p.x + half) / CELL).floor() as i32).clamp(0, n - 1) + (((p.y + half) / CELL).floor() as i32).clamp(0, n - 1) * n;
        // The nearest wet cell to a point (a berth may sit in a cell whose
        // centre is on the bank).
        let snap = |p: Vec2| -> Option<i32> {
            let c = cell_of(p);
            let (ci, cj) = (c % n, c / n);
            let mut best: Option<(f32, i32)> = None;
            for dj in -4..=4 {
                for di in -4..=4 {
                    let (i, j) = (ci + di, cj + dj);
                    if i < 0 || j < 0 || i >= n || j >= n || !wet[(i + j * n) as usize] {
                        continue;
                    }
                    let d = centre(i, j).distance(p);
                    if best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, i + j * n));
                    }
                }
            }
            best.map(|b| b.1)
        };
        let (Some(start), Some(goal)) = (snap(a), snap(b)) else { return vec![a, b] };
        // A* over the cells, eight ways (no corner cutting).
        let total = (n * n) as usize;
        let mut cost = vec![u32::MAX; total];
        let mut prev = vec![u32::MAX; total];
        let (gi, gj) = (goal % n, goal / n);
        let h = |k: i32| {
            let (di, dj) = (((k % n) - gi).unsigned_abs(), ((k / n) - gj).unsigned_abs());
            10 * di.max(dj) + 4 * di.min(dj)
        };
        let mut open = std::collections::BinaryHeap::new();
        cost[start as usize] = 0;
        open.push(std::cmp::Reverse((h(start), start)));
        while let Some(std::cmp::Reverse((_, k))) = open.pop() {
            if k == goal {
                break;
            }
            let (i, j) = (k % n, k / n);
            for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let (x, y) = (i + di, j + dj);
                if x < 0 || y < 0 || x >= n || y >= n || !wet[(x + y * n) as usize] {
                    continue;
                }
                if di != 0 && dj != 0 && !(wet[(x + j * n) as usize] && wet[(i + y * n) as usize]) {
                    continue;
                }
                let m = x + y * n;
                let c = cost[k as usize] + if di != 0 && dj != 0 { 14 } else { 10 };
                if c < cost[m as usize] {
                    cost[m as usize] = c;
                    prev[m as usize] = k as u32;
                    open.push(std::cmp::Reverse((c + h(m), m)));
                }
            }
        }
        if cost[goal as usize] == u32::MAX {
            return vec![a, b];
        }
        let mut cells = vec![goal];
        while let Some(&c) = cells.last() {
            let p = prev[c as usize];
            if p == u32::MAX {
                break;
            }
            cells.push(p as i32);
        }
        cells.reverse();
        let mut pts = vec![a];
        pts.extend(cells.iter().map(|&k| centre(k % n, k / n)));
        pts.push(b);
        // Pull it taut: from each corner, straight to the farthest point
        // still in clear water.
        let mut out = vec![a];
        let mut i = 0;
        while i + 1 < pts.len() {
            let mut j = pts.len() - 1;
            while j > i + 1 && !self.clear_water(pts[i], pts[j]) {
                j -= 1;
            }
            out.push(pts[j]);
            i = j;
        }
        out
    }
}

/// [ferries] A side of the map, for routes that leave it. `+y` is north
/// (the sea's `SeaExit::South` leaves towards `-y`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MapEdge {
    North,
    East,
    South,
    West,
}

impl MapEdge {
    pub const ALL: [MapEdge; 4] = [MapEdge::North, MapEdge::East, MapEdge::South, MapEdge::West];

    /// The side a point far out (or a direction) lies towards.
    pub fn of(p: Vec2) -> MapEdge {
        if p.x.abs() >= p.y.abs() {
            if p.x >= 0.0 { MapEdge::East } else { MapEdge::West }
        } else if p.y >= 0.0 {
            MapEdge::North
        } else {
            MapEdge::South
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MapEdge::North => "north",
            MapEdge::East => "east",
            MapEdge::South => "south",
            MapEdge::West => "west",
        }
    }
}

/// Nodes along every river centreline; edges along them and where a
/// tributary meets the river it flows into. `exit` marks river ends at the
/// edge of the map; `sea` marks nodes out in the sea.
pub(super) struct WaterGraph {
    pub(super) nodes: Vec<Vec2>,
    pub(super) adj: Vec<Vec<usize>>,
    pub(super) exit: Vec<bool>,
    pub(super) sea: Vec<bool>,
}

impl WaterGraph {
    pub(super) fn build(l: &Landscape) -> WaterGraph {
        let mut g = WaterGraph { nodes: Vec::new(), adj: Vec::new(), exit: Vec::new(), sea: Vec::new() };
        let mut ranges = Vec::new();
        for r in &l.rivers {
            let start = g.nodes.len();
            for (k, p) in r.points.iter().enumerate() {
                g.nodes.push(*p);
                g.adj.push(Vec::new());
                let edge = p.x.abs() >= LAND_HALF - 30.0 || p.y.abs() >= LAND_HALF - 30.0;
                g.exit.push(edge && (k == 0 || k + 1 == r.points.len()));
                g.sea.push(l.sea.as_ref().is_some_and(|s| s.contains(*p)));
                if k > 0 {
                    let i = g.nodes.len() - 1;
                    g.adj[i].push(i - 1);
                    g.adj[i - 1].push(i);
                }
            }
            ranges.push(start..g.nodes.len());
        }
        // Ends of one river that lie in another join it.
        for (ri, r) in l.rivers.iter().enumerate() {
            for end in [ranges[ri].start, ranges[ri].end - 1] {
                let p = g.nodes[end];
                for (rj, o) in l.rivers.iter().enumerate() {
                    if rj == ri {
                        continue;
                    }
                    let (seg, _, d) = o.nearest(p);
                    if d <= o.width * 0.5 + r.width * 0.5 + 25.0 {
                        let k = ranges[rj].start + seg;
                        g.adj[end].push(k);
                        g.adj[k].push(end);
                    }
                }
            }
        }
        g
    }

    pub(super) fn nearest(&self, l: &Landscape, p: Vec2) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for (ri, r) in l.rivers.iter().enumerate() {
            let (seg, _, d) = r.nearest(p);
            if d <= r.width * 0.5 + 80.0 && best.is_none_or(|(bd, _)| d < bd) {
                let base: usize = l.rivers[..ri].iter().map(|r| r.points.len()).sum();
                best = Some((d, base + seg));
            }
        }
        best.map(|(_, i)| i)
    }

    /// Dijkstra from `start` until `goal(node)`. Distances are exact (a
    /// rounded key once let a zero-length edge point two nodes at each other
    /// and the path never ended).
    pub(super) fn search(&self, start: usize, goal: impl Fn(usize) -> bool) -> Option<Vec<usize>> {
        #[derive(PartialEq)]
        struct Item(f32, usize);
        impl Eq for Item {}
        impl PartialOrd for Item {
            fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for Item {
            fn cmp(&self, o: &Self) -> std::cmp::Ordering {
                o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
            }
        }
        let n = self.nodes.len();
        let mut dist = vec![f32::INFINITY; n];
        let mut prev = vec![usize::MAX; n];
        let mut done = vec![false; n];
        let mut heap = std::collections::BinaryHeap::new();
        dist[start] = 0.0;
        heap.push(Item(0.0, start));
        while let Some(Item(du, u)) = heap.pop() {
            if done[u] {
                continue;
            }
            done[u] = true;
            if goal(u) {
                let mut path = vec![u];
                let mut c = u;
                while prev[c] != usize::MAX && path.len() <= n {
                    c = prev[c];
                    path.push(c);
                }
                path.reverse();
                return Some(path);
            }
            for &v in &self.adj[u] {
                let nd = du + self.nodes[u].distance(self.nodes[v]);
                if !done[v] && nd < dist[v] {
                    dist[v] = nd;
                    prev[v] = u;
                    heap.push(Item(nd, v));
                }
            }
        }
        None
    }

    /// Shortest distances from `start` to every node (Dijkstra over the
    /// whole graph) and the tree to walk paths back along.
    pub(super) fn tree(&self, start: usize) -> (Vec<f32>, Vec<usize>) {
        let n = self.nodes.len();
        let mut dist = vec![f32::INFINITY; n];
        let mut prev = vec![usize::MAX; n];
        let mut done = vec![false; n];
        dist[start] = 0.0;
        // Small graphs (a few hundred nodes): a plain scan is plenty.
        loop {
            let mut u = usize::MAX;
            for k in 0..n {
                if !done[k] && dist[k].is_finite() && (u == usize::MAX || dist[k] < dist[u]) {
                    u = k;
                }
            }
            if u == usize::MAX {
                break;
            }
            done[u] = true;
            for &v in &self.adj[u] {
                let nd = dist[u] + self.nodes[u].distance(self.nodes[v]);
                if nd < dist[v] {
                    dist[v] = nd;
                    prev[v] = u;
                }
            }
        }
        (dist, prev)
    }

    /// [ferries] The nearest way out across each edge of the map.
    pub(super) fn routes_out(&self, l: &Landscape, from: Vec2) -> Vec<(MapEdge, Vec<Vec2>)> {
        let in_sea = |p: Vec2| l.sea.as_ref().is_some_and(|s| s.contains(p));
        if in_sea(from) {
            let Some(sea) = l.sea.as_ref() else { return Vec::new() };
            let out = sea.exit_from(from);
            return vec![(MapEdge::of(out), l.sea_way(from, out))];
        }
        let Some(start) = self.nearest(l, from) else { return Vec::new() };
        let (dist, prev) = self.tree(start);
        // The way out from each goal node: on past the river's end, or
        // across the sea from its mouth.
        let mut best: Vec<(MapEdge, f32, usize, Vec2)> = Vec::new();
        for k in 0..self.nodes.len() {
            if !dist[k].is_finite() || !(self.exit[k] || self.sea[k]) {
                continue;
            }
            let p = self.nodes[k];
            let out = if self.sea[k] {
                match l.sea.as_ref() {
                    Some(s) => s.exit_from(p),
                    None => continue,
                }
            } else {
                p
            };
            let edge = MapEdge::of(out);
            let d = dist[k] + p.distance(out);
            match best.iter_mut().find(|b| b.0 == edge) {
                Some(b) if b.1 <= d => {}
                Some(b) => *b = (edge, d, k, out),
                None => best.push((edge, d, k, out)),
            }
        }
        best.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        best.into_iter()
            .map(|(edge, _, k, out)| {
                let mut path = vec![k];
                let mut c = k;
                while prev[c] != usize::MAX && path.len() <= self.nodes.len() {
                    c = prev[c];
                    path.push(c);
                }
                path.reverse();
                let mut pts = vec![from];
                pts.extend(path.iter().map(|&i| self.nodes[i]));
                if self.sea[k] {
                    let last = *pts.last().unwrap_or(&from);
                    pts.extend(l.sea_way(last, out).into_iter().skip(1));
                } else {
                    // Carry on past the edge, as `route` does.
                    let q = path.len().checked_sub(2).map_or(out, |i| self.nodes[path[i]]);
                    pts.push(out + (out - q).normalize_or(out.normalize_or(Vec2::X)) * 120.0);
                }
                (edge, pts)
            })
            .collect()
    }

    pub(super) fn route(&self, l: &Landscape, from: Vec2, to: Option<Vec2>) -> Option<Vec<Vec2>> {
        let in_sea = |p: Vec2| l.sea.as_ref().is_some_and(|s| s.contains(p));
        let lake_of = |p: Vec2| l.lakes.iter().position(|k| point_in_polygon(p, &k.outline));
        // The sea and lakes are crossed straight.
        if let Some(t) = to {
            if in_sea(from) && in_sea(t) {
                return Some(l.sea_way(from, t));
            }
            if lake_of(from).is_some_and(|a| lake_of(t) == Some(a)) {
                return Some(vec![from, t]);
            }
        } else if in_sea(from) {
            return Some(l.sea_way(from, l.sea.as_ref()?.exit_from(from)));
        }
        let start = self.nearest(l, from);
        let mut out = vec![from];
        let Some(start) = start else {
            // Out in the sea with a river goal: straight to its mouth below.
            if in_sea(from) {
                if let Some(t) = to {
                    let goal = self.nearest(l, t)?;
                    let path = self.search(goal, |k| self.sea[k])?;
                    out.extend(path.iter().rev().map(|&k| self.nodes[k]));
                    out.push(t);
                    return Some(out);
                }
            }
            return None;
        };
        match to {
            None => {
                let path = self.search(start, |k| self.exit[k] || self.sea[k])?;
                out.extend(path.iter().map(|&k| self.nodes[k]));
                let last = *path.last()?;
                if self.sea[last] {
                    let p = self.nodes[last];
                    out.extend(l.sea_way(p, l.sea.as_ref()?.exit_from(p)).into_iter().skip(1));
                } else {
                    // Carry on past the edge.
                    let p = self.nodes[last];
                    let q = self.nodes[*path.get(path.len().wrapping_sub(2)).unwrap_or(&last)];
                    out.push(p + (p - q).normalize_or(p.normalize_or(Vec2::X)) * 120.0);
                }
            }
            Some(t) => {
                if in_sea(t) {
                    let path = self.search(start, |k| self.sea[k])?;
                    out.extend(path.iter().map(|&k| self.nodes[k]));
                    let last = *out.last().unwrap_or(&from);
                    out.extend(l.sea_way(last, t).into_iter().skip(1));
                } else {
                    let goal = self.nearest(l, t)?;
                    let path = self.search(start, |k| k == goal)?;
                    out.extend(path.iter().map(|&k| self.nodes[k]));
                    out.push(t);
                }
            }
        }
        Some(out)
    }
}
