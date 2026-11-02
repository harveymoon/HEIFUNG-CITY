//! Walking routes (A* over the walk graph, jaywalking optional), nearest
//! nodes and distance fields.

use super::*;

/// A* queue entry: (estimate, cost so far, node), popped lowest estimate first.
#[derive(PartialEq)]
struct AstarItem(f32, f32, u32);
impl Eq for AstarItem {}
impl PartialOrd for AstarItem {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for AstarItem {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.0.total_cmp(&self.0)
    }
}

/// [r6-perf] [`WalkGraph::route_nodes_with`]'s tables, kept between searches:
/// every `dist` is infinite and every `prev` unset between calls.
#[derive(Default)]
struct AstarScratch {
    dist: Vec<f32>,
    prev: Vec<u32>,
    heap: BinaryHeap<AstarItem>,
    touched: Vec<u32>,
}

/// [r6-perf] Walking routes already searched, by (from, to, jaywalk), for
/// as long as nothing a search reads has changed: commuters walk the same
/// way every day (about half of a big city's searches repeat within six
/// hours). Kept only while `WalkGraph::route_stamp` matches the graph (its
/// version, size, shut lots and every crossing's route cost), and cleared
/// by the graph's own edits ([`WalkGraph::forget_routes`]), so a hit is
/// the route the search would have found. Not saved; a clone starts empty.
#[derive(Default)]
pub(super) struct RouteMemo(std::sync::Mutex<MemoInner>);

#[derive(Default)]
struct MemoInner {
    stamp: u64,
    held: usize,
    routes: HashMap<(u32, u32, bool), Option<std::sync::Arc<[u32]>>>,
    /// The packed graph, rebuilt when the version or size changes or an
    /// edit clears it.
    index: Option<std::sync::Arc<RouteIndex>>,
    /// `crossing_costs` at `stamp`.
    costs: Option<std::sync::Arc<[Vec<f32>; 2]>>,
}

/// Routes, and node ids across them, held before the memo starts over.
const MEMO_ROUTES: usize = 40_000;
const MEMO_NODES: usize = 4_000_000;

impl Clone for RouteMemo {
    /// Empty, but for [`crate::versioned::exact_clone`]: the same routes,
    /// shared.
    fn clone(&self) -> Self {
        if crate::versioned::cloning_exact() {
            if let Ok(m) = self.0.lock() {
                return RouteMemo(std::sync::Mutex::new(MemoInner { stamp: m.stamp, held: m.held, routes: m.routes.clone(), index: m.index.clone(), costs: m.costs.clone() }));
            }
        }
        RouteMemo::default()
    }
}

impl std::fmt::Debug for RouteMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.0.lock().map_or(0, |m| m.routes.len());
        write!(f, "RouteMemo({n} routes)")
    }
}

/// [r6-perf] `ND_VERIFY_ROUTES=1`: every route is searched again by the
/// plain A* and must cost the same (`strict`: be the same), and every memo
/// hit must be what a fresh search finds (a check for an edit that forgot
/// to clear the memo).
pub(crate) fn verify_routes() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| std::env::var_os("ND_VERIFY_ROUTES").is_some_and(|v| v != "0"))
}

/// [r6-perf] The landmark bound is shrunk by this much, so that rounding in
/// the f32 sums of a long route never lets it overestimate.
const LANDMARK_SLACK: f32 = 0.999;

/// [r6-perf] Landmarks: nodes spread round the walk graph's largest
/// component, with every node's walking cost from each.
const LANDMARKS: usize = 8;

/// The cost factor of an edge, before any crossing's extra.
fn edge_factor(e: &WalkEdge) -> f32 {
    if e.park {
        PARK_FACTOR
    } else if e.interior {
        INTERIOR_FACTOR
    } else {
        1.0
    }
}

/// The route to `to` from a finished search's tables.
fn trace(dist: &[f32], prev: &[u32], from: u32, to: u32) -> Option<Vec<u32>> {
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

/// [r6-perf] The walk graph packed for searching: every node's edges side
/// by side (in `adj`'s order) with their cost factor applied, the node
/// positions, and the landmark costs (ALT: a landmark's cost to the goal
/// less its cost to a node is a lower bound on the node's cost to the
/// goal, far tighter than the straight line through a city's blocks).
/// Landmark costs are over every edge at its factor alone, every crossing
/// open and free, so they never exceed a real route's cost.
struct RouteIndex {
    version: u64,
    offs: Vec<u32>,
    to: Vec<u32>,
    cost: Vec<f32>,
    cross: Vec<u32>,
    /// A park walkway or grounds gate (`WalkGraph::shut` when its lot is).
    gated: Vec<bool>,
    pos: Vec<Vec2>,
    /// `landmarks` costs per node, node by node; `INFINITY` out of reach.
    lm: Vec<f32>,
    landmarks: usize,
    /// Every edge has its twin back at the same cost: the bound holds
    /// both ways round.
    symmetric: bool,
}

impl RouteIndex {
    fn build(g: &WalkGraph) -> Self {
        crate::prof_scope!("walk_route_index");
        let n = g.nodes.len();
        let mut offs = Vec::with_capacity(n + 1);
        let (mut to, mut cost, mut cross, mut gated) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        offs.push(0u32);
        for es in &g.adj {
            for e in es {
                to.push(e.to);
                cost.push(e.len * edge_factor(e));
                cross.push(e.cross);
                gated.push(e.park || e.gate);
            }
            offs.push(to.len() as u32);
        }
        let pos: Vec<Vec2> = g.nodes.iter().map(|x| x.pos).collect();
        let mut x = RouteIndex { version: g.version, offs, to, cost, cross, gated, pos, lm: Vec::new(), landmarks: 0, symmetric: false };
        x.symmetric = (0..n).all(|u| {
            (x.offs[u] as usize..x.offs[u + 1] as usize).all(|k| {
                let v = x.to[k] as usize;
                (x.offs[v] as usize..x.offs[v + 1] as usize).any(|j| x.to[j] as usize == u && x.cost[j] == x.cost[k])
            })
        });
        x.place_landmarks(g);
        x
    }

    /// Every node's cost from `src` over all edges at their factor (in
    /// f64, so the sums are as close as can be).
    fn costs_from(&self, src: u32) -> Vec<f64> {
        #[derive(PartialEq)]
        struct Item(f64, u32);
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
        let n = self.pos.len();
        let mut dist = vec![f64::INFINITY; n];
        let mut heap = BinaryHeap::new();
        dist[src as usize] = 0.0;
        heap.push(Item(0.0, src));
        while let Some(Item(d, u)) = heap.pop() {
            if d > dist[u as usize] {
                continue;
            }
            for k in self.offs[u as usize] as usize..self.offs[u as usize + 1] as usize {
                let v = self.to[k] as usize;
                let nd = d + self.cost[k] as f64;
                if nd < dist[v] {
                    dist[v] = nd;
                    heap.push(Item(nd, v as u32));
                }
            }
        }
        dist
    }

    /// Farthest-first: the first landmark is the node farthest from the
    /// largest component's lowest node, each next the node farthest from
    /// all chosen so far. Deterministic (ties to the lower index).
    fn place_landmarks(&mut self, g: &WalkGraph) {
        let n = self.pos.len();
        if n < 64 {
            return;
        }
        let start = if g.comp.len() == n {
            let mut size: HashMap<u32, u32> = HashMap::new();
            for &c in &g.comp {
                *size.entry(c).or_default() += 1;
            }
            size.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0))).map_or(0, |(c, _)| c)
        } else {
            0
        };
        let farthest = |d: &[f64]| (0..n).filter(|&i| d[i].is_finite()).max_by(|&a, &b| d[a].total_cmp(&d[b]).then(b.cmp(&a)));
        let mut near = self.costs_from(start);
        let mut lms: Vec<Vec<f64>> = Vec::new();
        for _ in 0..landmark_count() {
            let Some(l) = farthest(&near) else { break };
            if near[l] <= 0.0 && !lms.is_empty() {
                break;
            }
            let d = self.costs_from(l as u32);
            if lms.is_empty() {
                near = d.clone();
            } else {
                for (a, b) in near.iter_mut().zip(&d) {
                    *a = a.min(*b);
                }
            }
            lms.push(d);
        }
        self.landmarks = lms.len();
        self.lm = vec![f32::INFINITY; n * self.landmarks];
        for (k, d) in lms.iter().enumerate() {
            for i in 0..n {
                if d[i].is_finite() {
                    self.lm[i * self.landmarks + k] = d[i] as f32;
                }
            }
        }
    }
}

/// [r6-perf] `ND_LANDMARKS=<n>` overrides [`LANDMARKS`]; 0 searches by the
/// straight-line bound alone, exactly as before the landmarks.
fn landmark_count() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| std::env::var("ND_LANDMARKS").ok().and_then(|v| v.parse().ok()).unwrap_or(LANDMARKS))
}

thread_local! {
    /// [r6-perf] Nodes popped by the searches on this thread, for the
    /// tests that guard how much a search costs.
    pub(super) static POPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static ASTAR: std::cell::RefCell<AstarScratch> = std::cell::RefCell::new(AstarScratch::default());
}

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
        let n = self.nodes.len();
        // [pt-walk] Different components: no route, and no search of the
        // whole of the one we start in to find that out.
        if self.comp.len() == n && self.comp[from as usize] != self.comp[to as usize] {
            return None;
        }
        let stamp = self.route_stamp();
        let (index, costs) = {
            let Ok(mut m) = self.routes.0.lock() else { return self.search_reference(from, to, jaywalk) };
            if m.stamp != stamp || m.costs.is_none() {
                m.routes.clear();
                m.held = 0;
                m.stamp = stamp;
                m.costs = Some(std::sync::Arc::new(self.crossing_costs()));
            }
            if m.index.as_ref().is_none_or(|x| x.version != self.version || x.offs.len() != n + 1) {
                m.index = Some(std::sync::Arc::new(RouteIndex::build(self)));
            }
            if let Some(hit) = m.routes.get(&(from, to, jaywalk)) {
                let hit = hit.as_ref().map(|r| r.to_vec());
                let (m_index, m_costs) = (m.index.clone().expect("built above"), m.costs.clone().expect("built above"));
                drop(m);
                if verify_routes() {
                    let (index, costs) = (m_index, m_costs);
                    assert_eq!(hit, self.search_indexed(&index, &costs[jaywalk as usize], from, to), "walk route memo is stale: {from} -> {to}");
                    self.check_route(&hit, from, to, jaywalk);
                }
                return hit;
            }
            (m.index.clone().expect("built above"), m.costs.clone().expect("built above"))
        };
        let found = self.search_indexed(&index, &costs[jaywalk as usize], from, to);
        if verify_routes() {
            self.check_route(&found, from, to, jaywalk);
        }
        if let Ok(mut m) = self.routes.0.lock() {
            let len = found.as_ref().map_or(1, |r| r.len());
            if m.routes.len() >= MEMO_ROUTES || m.held + len > MEMO_NODES {
                m.routes.clear();
                m.held = 0;
            }
            m.held += len;
            m.routes.insert((from, to, jaywalk), found.as_deref().map(std::sync::Arc::from));
        }
        found
    }

    /// [r6-perf] Everything a search reads that can change without a new
    /// graph, hashed: the version and size, the shut lots, and every
    /// crossing's cost both ways (a few thousand, against the thousands of
    /// nodes a search pops).
    fn route_stamp(&self) -> u64 {
        crate::prof_scope!("walk_route_stamp");
        let mut h: u64 = 0xcbf29ce484222325;
        let mut mix = |x: u64| {
            h = (h ^ x).wrapping_mul(0x100000001b3);
        };
        mix(self.version);
        mix(self.nodes.len() as u64);
        mix(self.crossings.len() as u64);
        mix(self.closed.len() as u64);
        for l in &self.closed {
            mix(slotmap::Key::data(l).as_ffi());
        }
        for c in &self.crossings {
            let lawful = c.route_cost(false).map_or(u32::MAX, f32::to_bits) as u64;
            let jay = c.route_cost(true).map_or(u32::MAX, f32::to_bits) as u64;
            mix(lawful << 32 | jay);
        }
        h
    }

    /// [r6-perf] Each crossing's `route_cost`, lawful and jaywalking, with
    /// `INFINITY` for one that may not be crossed.
    fn crossing_costs(&self) -> [Vec<f32>; 2] {
        [false, true].map(|jay| self.crossings.iter().map(|c| c.route_cost(jay).unwrap_or(f32::INFINITY)).collect())
    }

    /// [r6-perf] Clear the route memo and the search index: call after any
    /// edit to the nodes or edges.
    pub(crate) fn forget_routes(&mut self) {
        if let Ok(m) = self.routes.0.get_mut() {
            *m = MemoInner::default();
        }
    }

    /// [r6-perf] The A* over the packed edges ([`RouteIndex`]) with the
    /// landmark bound. It finds a cheapest route, as the plain search does;
    /// the two could only part where two routes cost exactly the same, and
    /// `ND_VERIFY_ROUTES` checks that they do not.
    fn search_indexed(&self, x: &RouteIndex, cross_cost: &[f32], from: u32, to: u32) -> Option<Vec<u32>> {
        crate::prof_scope!("walk_astar");
        let n = self.nodes.len();
        let goal = x.pos[to as usize];
        let nl = x.landmarks;
        let goal_lm: Vec<f32> = x.lm[to as usize * nl..(to as usize + 1) * nl].to_vec();
        let h = |i: u32| {
            let mut b = x.pos[i as usize].distance(goal) * PARK_FACTOR;
            let row = &x.lm[i as usize * nl..(i as usize + 1) * nl];
            for (&a, &t) in row.iter().zip(&goal_lm) {
                if a.is_finite() && t.is_finite() {
                    let d = if x.symmetric { (a - t).abs() } else { t - a };
                    b = b.max(d * LANDMARK_SLACK);
                }
            }
            b
        };
        // [under-construction] Park walkways and gates of a site are shut,
        // unless the walk starts in there (someone leaving).
        let keep_out = !self.closed.is_empty() && !self.node_closed(from);
        ASTAR.with(|cell| {
            let mut guard = cell.borrow_mut();
            let s = &mut *guard;
            if s.dist.len() != n {
                s.dist.clear();
                s.dist.resize(n, f32::INFINITY);
                s.prev.clear();
                s.prev.resize(n, u32::MAX);
            }
            s.heap.clear();
            let AstarScratch { dist, prev, heap, touched } = s;
            dist[from as usize] = 0.0;
            touched.push(from);
            heap.push(AstarItem(h(from), 0.0, from));
            while let Some(AstarItem(_, g, u)) = heap.pop() {
                POPS.with(|p| p.set(p.get() + 1));
                // Stale: `u` was reached more cheaply since this was pushed.
                if g > dist[u as usize] {
                    continue;
                }
                if u == to {
                    break;
                }
                let du = dist[u as usize];
                for k in x.offs[u as usize] as usize..x.offs[u as usize + 1] as usize {
                    let v = x.to[k];
                    if keep_out && x.gated[k] && self.node_closed(v) {
                        continue;
                    }
                    let mut cost = x.cost[k];
                    let c = x.cross[k];
                    if c != NO_CROSSING {
                        let extra = cross_cost[c as usize];
                        if extra == f32::INFINITY {
                            continue;
                        }
                        cost += extra;
                    }
                    let nd = du + cost;
                    if nd < dist[v as usize] {
                        if dist[v as usize] == f32::INFINITY {
                            touched.push(v);
                        }
                        dist[v as usize] = nd;
                        prev[v as usize] = u;
                        heap.push(AstarItem(nd + h(v), nd, v));
                    }
                }
            }
            let found = trace(dist, prev, from, to);
            for &i in touched.iter() {
                dist[i as usize] = f32::INFINITY;
                prev[i as usize] = u32::MAX;
            }
            touched.clear();
            found
        })
    }

    /// [r6-perf] `ND_VERIFY_ROUTES`: `found` against the plain A*. Both
    /// find a cheapest route; where several cost exactly the same the
    /// landmark bound may pick another of them, so routes must match in
    /// cost (and exactly with `ND_VERIFY_ROUTES=strict`, or `ND_LANDMARKS=0`).
    fn check_route(&self, found: &Option<Vec<u32>>, from: u32, to: u32, jaywalk: bool) {
        let want = self.search_reference(from, to, jaywalk);
        if *found == want {
            return;
        }
        let cost = |r: &Option<Vec<u32>>| r.as_ref().map(|r| self.path_cost(r, jaywalk));
        let (a, b) = (cost(found), cost(&want));
        let strict = std::env::var_os("ND_VERIFY_ROUTES").is_some_and(|v| v == "strict");
        let same_cost = match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() <= 1e-4 * b.max(1.0),
            _ => false,
        };
        if strict || !same_cost {
            panic!("walk route search differs from the plain A*: {from} -> {to} (jaywalk {jaywalk}): cost {a:?} ({} nodes) against {b:?} ({} nodes)", found.as_ref().map_or(0, |r| r.len()), want.as_ref().map_or(0, |r| r.len()));
        }
    }

    /// What a route costs a search, edge by edge (the cheapest edge between
    /// each two nodes): for the verification's messages.
    pub(super) fn path_cost(&self, r: &[u32], jaywalk: bool) -> f32 {
        let mut total = 0.0;
        for w in r.windows(2) {
            let c = self.adj[w[0] as usize]
                .iter()
                .filter(|e| e.to == w[1])
                .filter_map(|e| {
                    let extra = if e.cross == NO_CROSSING { Some(0.0) } else { self.crossings[e.cross as usize].route_cost(jaywalk) };
                    extra.map(|x| e.len * edge_factor(e) + x)
                })
                .fold(f32::INFINITY, f32::min);
            total += c;
        }
        total
    }

    /// The plain A* over `adj` with the straight-line bound only: what
    /// [`route_nodes_with`](Self::route_nodes_with) answered before the
    /// index, kept to check it against (`ND_VERIFY_ROUTES`).
    pub(super) fn search_reference(&self, from: u32, to: u32, jaywalk: bool) -> Option<Vec<u32>> {
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
        let goal = self.nodes[to as usize].pos;
        let h = |i: u32| self.nodes[i as usize].pos.distance(goal) * PARK_FACTOR;
        let mut dist = vec![f32::INFINITY; n];
        let mut prev = vec![u32::MAX; n];
        let mut heap = BinaryHeap::new();
        dist[from as usize] = 0.0;
        heap.push(Item(h(from), 0.0, from));
        let keep_out = !self.closed.is_empty() && !self.node_closed(from);
        while let Some(Item(_, g, u)) = heap.pop() {
            POPS.with(|p| p.set(p.get() + 1));
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
                let mut cost = e.len * edge_factor(e);
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
        trace(&dist, &prev, from, to)
    }

    /// Positions of a walking route between two points, snapping both ends
    /// to the network.
    pub fn route(&self, from: Vec2, to: Vec2) -> Option<(Vec<Vec2>, Vec<u32>)> {
        self.route_with(from, to, false)
    }

    /// [`route`](Self::route), letting a jaywalker take `Jay` edges.
    pub fn route_with(&self, from: Vec2, to: Vec2, jaywalk: bool) -> Option<(Vec<Vec2>, Vec<u32>)> {
        let (a, b) = crate::prof!("walk_snap", (self.snap_end(from), self.snap_end(to)));
        let (a, b) = (a?, b?);
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
        self.forget_routes();
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
