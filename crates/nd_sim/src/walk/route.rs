//! Walking routes (A* over the walk graph, jaywalking optional), nearest
//! nodes and distance fields.

use super::*;

impl WalkGraph {
    /// The crossing a walker takes going from `p` to `q`, two consecutive
    /// points of a route (node positions, exactly).
    pub fn crossing_between(&self, p: Vec2, q: Vec2) -> Option<u32> {
        if self.cross_at.is_empty() {
            return None;
        }
        self.cross_at.get(&pos_key(p, q)).copied()
    }

    /// Nearest node within `max_dist`; exact ties go to the lower index.
    pub fn nearest(&self, p: Vec2, max_dist: f32) -> Option<u32> {
        // Raised walkways are reached by their stairs, never snapped onto.
        let within = |i: usize| {
            let d = self.nodes[i].pos.distance(p);
            if d <= max_dist && self.nodes[i].level == 0.0 { d } else { f32::INFINITY }
        };
        if self.index.len() == self.nodes.len() {
            return self.index.nearest(p, |i| within(i as usize)).map(|(i, _)| i);
        }
        let mut best: Option<(f32, u32)> = None;
        for i in 0..self.nodes.len() {
            let d = within(i);
            if d.is_finite() && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i as u32));
            }
        }
        best.map(|(_, i)| i)
    }

    /// [r4-metroloop] The node a walk from (or to) `p` starts (or ends) at:
    /// the nearest, unless that is up on a deck (`WalkNode::deck`) while
    /// `p` is out from under it, on the ground: then the nearest that is
    /// not (any node if none is in reach). A metro rider setting off from
    /// the platform under Harvey's Kessler Drive viaduct was snapped onto
    /// its pavement 12 m up, walked it round for 40 minutes and came back
    /// down to the same station. A point at a node itself (a walk planned
    /// again on its way) stays at that node, deck or not.
    pub fn snap_end(&self, p: Vec2) -> Option<u32> {
        let n = self.nearest(p, 60.0)?;
        let node = &self.nodes[n as usize];
        if !node.deck || node.pos.distance(p) < 0.01 || self.under_deck(p) {
            return Some(n);
        }
        let within = |i: u32| {
            let x = &self.nodes[i as usize];
            let d = x.pos.distance(p);
            if d <= 60.0 && x.level == 0.0 && !x.deck { d } else { f32::INFINITY }
        };
        let ground = if self.index.len() == self.nodes.len() {
            self.index.nearest(p, within).map(|(i, _)| i)
        } else {
            (0..self.nodes.len() as u32).filter(|&i| within(i).is_finite()).min_by(|&x, &y| within(x).total_cmp(&within(y)).then(x.cmp(&y)))
        };
        Some(ground.unwrap_or(n))
    }

    /// [r4-metroloop] Is `p` on or under a deck (within its footprint,
    /// where the way stands `VIADUCT_LEVEL` or more up)? Only there can a
    /// point be up on one; in 2D the ground beneath looks the same.
    pub fn under_deck(&self, p: Vec2) -> bool {
        self.decks.iter().any(|&(a, b, la, lb, reach)| {
            let (t, q) = geom::project_on_segment(p, a, b);
            q.distance(p) <= reach && la + (lb - la) * t >= crate::roads::VIADUCT_LEVEL
        })
    }

    /// [ped-highway] Nearest node at ground level within `max_dist` that
    /// has an edge and passes `keep`; exact ties go to the lower index.
    pub fn nearest_matching(&self, p: Vec2, max_dist: f32, keep: impl Fn(u32) -> bool) -> Option<u32> {
        let within = |i: u32| {
            let n = &self.nodes[i as usize];
            let d = n.pos.distance(p);
            if d <= max_dist && n.level == 0.0 && !self.adj[i as usize].is_empty() && keep(i) { d } else { f32::INFINITY }
        };
        if self.index.len() == self.nodes.len() {
            return self.index.nearest(p, within).map(|(i, _)| i);
        }
        let mut best: Option<(f32, u32)> = None;
        for i in 0..self.nodes.len() as u32 {
            let d = within(i);
            if d.is_finite() && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// [saves] [`nearest_matching`](Self::nearest_matching) by a box query
    /// of `r` (same answer, exact ties to the lower index): asking about
    /// open country or the sea, where nothing is that close, no longer
    /// searches the whole map ring by ring. The land value's walk-time pass
    /// asks it for every grid cell: on a 17,000-person city that search
    /// was a 1.2 s tick every two game hours.
    pub fn nearest_matching_within(&self, p: Vec2, r: f32, keep: impl Fn(u32) -> bool) -> Option<u32> {
        if self.index.len() != self.nodes.len() {
            return self.nearest_matching(p, r, keep);
        }
        let mut best: Option<(f32, u32)> = None;
        for i in self.index.in_box(p - Vec2::splat(r), p + Vec2::splat(r)) {
            let n = &self.nodes[i as usize];
            let d = n.pos.distance(p);
            if d <= r && n.level == 0.0 && !self.adj[i as usize].is_empty() && keep(i) && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// Nearest node within `r` by a box query: unlike [`nearest`](Self::nearest)
    /// it never searches further out when nothing is that close, so asking
    /// about open country is as cheap as asking about a street. Exact ties
    /// go to the lower index.
    pub fn nearest_within(&self, p: Vec2, r: f32) -> Option<u32> {
        if self.index.len() != self.nodes.len() {
            return self.nearest(p, r);
        }
        let mut best: Option<(f32, u32)> = None;
        for i in self.index.in_box(p - Vec2::splat(r), p + Vec2::splat(r)) {
            let d = self.nodes[i as usize].pos.distance(p);
            if d <= r && self.nodes[i as usize].level == 0.0 && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// [under-construction] Is node `n` on a lot whose building is not open
    /// yet (`closed`)?
    pub fn node_closed(&self, n: u32) -> bool {
        !self.closed.is_empty() && self.nodes.get(n as usize).and_then(|x| x.lot).is_some_and(|l| self.closed.binary_search(&l).is_ok())
    }

    /// Is the edge `e` a park walkway or grounds gate into a shut lot?
    /// Nobody walks in; whoever was inside when it shut walks out.
    pub(super) fn shut(&self, e: &WalkEdge) -> bool {
        (e.park || e.gate) && self.node_closed(e.to)
    }

    /// A* between two nodes with the edge costs of [`PARK_FACTOR`] and
    /// [`INTERIOR_FACTOR`]. The heuristic is the straight-line distance
    /// scaled by the cheapest factor, so it never overestimates and the
    /// first time the goal pops it is optimal.
    pub fn route_nodes(&self, from: u32, to: u32) -> Option<Vec<u32>> {
        self.route_nodes_with(from, to, false)
    }

    /// [`route_nodes`](Self::route_nodes), over `Jay` edges too when
    /// `jaywalk` (each costs [`JAY_PENALTY`] metres more than its length).
    pub fn route_nodes_with(&self, from: u32, to: u32, jaywalk: bool) -> Option<Vec<u32>> {
        #[derive(PartialEq)]
        struct Item(f32, f32, u32);
        impl Eq for Item {}
        impl PartialOrd for Item {
            fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for Item {
            fn cmp(&self, o: &Self) -> std::cmp::Ordering {
                o.0.total_cmp(&self.0)
            }
        }
        let n = self.nodes.len();
        // [pt-walk] Different components: no route, and no search of the
        // whole of the one we start in to find that out.
        if self.comp.len() == n && self.comp[from as usize] != self.comp[to as usize] {
            return None;
        }
        let goal = self.nodes[to as usize].pos;
        let h = |i: u32| self.nodes[i as usize].pos.distance(goal) * PARK_FACTOR;
        let mut dist = vec![f32::INFINITY; n];
        let mut prev = vec![u32::MAX; n];
        let mut heap = BinaryHeap::new();
        dist[from as usize] = 0.0;
        heap.push(Item(h(from), 0.0, from));
        // [under-construction] Park walkways and gates of a site are shut,
        // unless the walk starts in there (someone leaving).
        let keep_out = !self.closed.is_empty() && !self.node_closed(from);
        while let Some(Item(_, g, u)) = heap.pop() {
            // Stale: `u` was reached more cheaply since this was pushed.
            if g > dist[u as usize] {
                continue;
            }
            if u == to {
                break;
            }
            let du = dist[u as usize];
            for e in &self.adj[u as usize] {
                if keep_out && self.shut(e) {
                    continue;
                }
                let mut cost = e.len * if e.park { PARK_FACTOR } else if e.interior { INTERIOR_FACTOR } else { 1.0 };
                if e.cross != NO_CROSSING {
                    match self.crossings[e.cross as usize].route_cost(jaywalk) {
                        Some(extra) => cost += extra,
                        None => continue,
                    }
                }
                let nd = du + cost;
                if nd < dist[e.to as usize] {
                    dist[e.to as usize] = nd;
                    prev[e.to as usize] = u;
                    heap.push(Item(nd + h(e.to), nd, e.to));
                }
            }
        }
        if !dist[to as usize].is_finite() {
            return None;
        }
        let mut path = vec![to];
        let mut cur = to;
        while cur != from {
            cur = prev[cur as usize];
            if cur == u32::MAX {
                return None;
            }
            path.push(cur);
        }
        path.reverse();
        Some(path)
    }

    /// Positions of a walking route between two points, snapping both ends
    /// to the network.
    pub fn route(&self, from: Vec2, to: Vec2) -> Option<(Vec<Vec2>, Vec<u32>)> {
        self.route_with(from, to, false)
    }

    /// [`route`](Self::route), letting a jaywalker take `Jay` edges.
    pub fn route_with(&self, from: Vec2, to: Vec2, jaywalk: bool) -> Option<(Vec<Vec2>, Vec<u32>)> {
        let a = self.snap_end(from)?;
        let b = self.snap_end(to)?;
        // [under-construction] Not to a spot inside a shut park: the
        // nearest node outside it instead.
        let b = if self.node_closed(b) && !self.node_closed(a) { self.nearest_matching(to, 60.0, |n| !self.node_closed(n)).unwrap_or(b) } else { b };
        let nodes = self.route_nodes_with(a, b, jaywalk)?;
        let pts = nodes.iter().map(|&i| self.nodes[i as usize].pos).collect();
        Some((pts, nodes))
    }

    /// Metres on foot from the nearest of `sources` to every node: one
    /// multi-source Dijkstra over the lawful edges (no jaywalking), plain
    /// lengths without the route preferences, `f32::INFINITY` where none of
    /// them can be reached. Used for how far a home is from the nearest
    /// park or night out (`LandValue::access_at`), so it costs one pass
    /// per kind of amenity however many there are.
    pub fn distances_from(&self, sources: &[u32]) -> Vec<f32> {
        #[derive(PartialEq)]
        struct Item(f32, u32);
        impl Eq for Item {}
        impl PartialOrd for Item {
            fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for Item {
            // Min-heap on distance; ties by node index so the order of
            // equal pops never depends on how the heap was filled.
            fn cmp(&self, o: &Self) -> std::cmp::Ordering {
                o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
            }
        }
        let mut dist = vec![f32::INFINITY; self.nodes.len()];
        let mut heap = BinaryHeap::new();
        for &s in sources {
            if (s as usize) < dist.len() && dist[s as usize] > 0.0 {
                dist[s as usize] = 0.0;
                heap.push(Item(0.0, s));
            }
        }
        while let Some(Item(d, u)) = heap.pop() {
            if d > dist[u as usize] {
                continue;
            }
            for e in &self.adj[u as usize] {
                if e.cross != NO_CROSSING && self.crossings[e.cross as usize].route_cost(false).is_none() {
                    continue;
                }
                if self.shut(e) {
                    continue;
                }
                let nd = d + e.len;
                if nd < dist[e.to as usize] {
                    dist[e.to as usize] = nd;
                    heap.push(Item(nd, e.to));
                }
            }
        }
        dist
    }

    /// [pt-walk] Label every node with its connected component (the
    /// lowest node index in it), over every edge in the graph. Call once
    /// the graph is finished; any later edge would need a fresh call.
    pub fn compute_components(&mut self) {
        let n = self.nodes.len();
        let mut parent: Vec<u32> = (0..n as u32).collect();
        fn find(parent: &mut [u32], mut x: u32) -> u32 {
            while parent[x as usize] != x {
                let p = parent[x as usize];
                parent[x as usize] = parent[p as usize];
                x = p;
            }
            x
        }
        for u in 0..n {
            for e in &self.adj[u] {
                let (a, b) = (find(&mut parent, u as u32), find(&mut parent, e.to));
                // The lower root wins, so a label is its component's lowest node.
                if a != b {
                    let (lo, hi) = (a.min(b), a.max(b));
                    parent[hi as usize] = lo;
                }
            }
        }
        self.comp = (0..n as u32).map(|i| find(&mut parent, i)).collect();
    }

    /// [pt-walk] Every node whose position lies in the box, by the grid
    /// (a scan when the node list has grown since it was built).
    pub fn nodes_in_box(&self, lo: Vec2, hi: Vec2) -> Vec<u32> {
        if self.index.len() == self.nodes.len() {
            return self.index.in_box(lo, hi);
        }
        (0..self.nodes.len() as u32).filter(|&i| {
            let p = self.nodes[i as usize].pos;
            p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y
        }).collect()
    }

    /// [pt-walk] Component of node `n` (None before `compute_components`).
    pub fn component(&self, n: u32) -> Option<u32> {
        (self.comp.len() == self.nodes.len()).then(|| self.comp.get(n as usize).copied()).flatten()
    }

    /// Lots passed by a route, each once.
    pub fn lots_passed(&self, nodes: &[u32]) -> Vec<LotId> {
        let mut out: Vec<LotId> = Vec::new();
        for &i in nodes {
            let n = &self.nodes[i as usize];
            for l in n.lot.iter().chain(n.near.iter()) {
                if !out.contains(l) {
                    out.push(*l);
                }
            }
        }
        out
    }
}
