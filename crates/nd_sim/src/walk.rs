//! The pedestrian network: pavements on both sides of every street, path
//! centres, crossings at junctions, a walkable setback loop around every
//! usable lot, an alley cross inside the Stacks, and gates between lots that
//! share an edge. Walkers route over this with A*, so people cut through
//! blocks and the shops they pass get walk-in trade.

use crate::geom::{self, Grid};
use crate::roads::{RoadKind, RoadNetwork, SegmentId};
use crate::zoning::{Lot, LotId, SIDEWALK};
use crate::{LandUse, LandValue, Tier};
use glam::Vec2;
use slotmap::SlotMap;
use std::collections::{BinaryHeap, HashMap};

/// Inset of the walkable loop from the parcel edge.
const LOOP_INSET: f32 = 1.2;
/// Longest shared boundary needed before two lots get a gate.
const GATE_MIN_SHARED: f32 = 4.0;
/// How far a lot's front node may be from the pavement to connect.
const FRONT_REACH: f32 = 14.0;
/// Cell size of the node index; pavement nodes are a lot's width apart.
const NODE_CELL: f32 = 20.0;
/// Route cost multipliers. Park paths are pleasant, so routes prefer them
/// a little; interior edges (alleys, loops, gates) cost a little more so
/// people do not thread through back yards when the pavement is as short.
const PARK_FACTOR: f32 = 0.92;
const INTERIOR_FACTOR: f32 = 1.15;

#[derive(Clone, Debug)]
pub struct WalkNode {
    pub pos: Vec2,
    /// Lot this node lies inside, for lot-interior edges.
    pub lot: Option<LotId>,
    /// Lots whose frontage this node passes, for footfall.
    pub near: Vec<LotId>,
}

#[derive(Clone, Copy, Debug)]
pub struct WalkEdge {
    pub to: u32,
    pub len: f32,
    /// Inside a lot (alley, setback, gate) rather than on a public street.
    pub interior: bool,
    /// A park path: pleasant, so routes prefer it a little.
    pub park: bool,
}

#[derive(Clone, Debug, Default)]
pub struct WalkGraph {
    pub nodes: Vec<WalkNode>,
    pub adj: Vec<Vec<WalkEdge>>,
    pub version: u64,
    /// Park benches: node, lot, and the direction a sitter faces.
    pub benches: Vec<(u32, LotId, f32)>,
    /// Grid over `nodes`, built at the end of `build`. `nearest` falls back
    /// to a scan if the node list has grown since.
    index: Grid,
}

/// Can people walk through this lot to the next one?
pub fn walkable_through(lot: &Lot, land_value: &LandValue) -> bool {
    let tier = Tier::for_land_value(land_value.at(lot.center));
    match lot.land_use {
        LandUse::Industrial => false,
        LandUse::Residential => tier != Tier::High,
        _ => true,
    }
}

impl WalkGraph {
    fn add_node(&mut self, pos: Vec2, lot: Option<LotId>) -> u32 {
        self.nodes.push(WalkNode { pos, lot, near: Vec::new() });
        self.adj.push(Vec::new());
        (self.nodes.len() - 1) as u32
    }

    fn link(&mut self, a: u32, b: u32, interior: bool) {
        self.link_kind(a, b, interior, false);
    }

    fn link_kind(&mut self, a: u32, b: u32, interior: bool, park: bool) {
        if a == b {
            return;
        }
        let len = self.nodes[a as usize].pos.distance(self.nodes[b as usize].pos).max(0.05);
        self.adj[a as usize].push(WalkEdge { to: b, len, interior, park });
        self.adj[b as usize].push(WalkEdge { to: a, len, interior, park });
    }

    pub fn build(roads: &RoadNetwork, lots: &SlotMap<LotId, Lot>, land_value: &LandValue, version: u64) -> Self {
        let mut g = WalkGraph { version, ..Default::default() };

        // ── streets and paths ────────────────────────────────────────────
        // Endpoints per road node: (node index, position, segment, side).
        let mut ends: Vec<Vec<(u32, SegmentId, i8)>> = vec![Vec::new(); roads.nodes.len()];
        let mut street_nodes: Vec<u32> = Vec::new();
        // Pavement node opposite every lot front, keyed by lot.
        let mut front_nodes: HashMap<LotId, u32> = HashMap::new();
        // Usable lots by the segment they front, in lot order, so each
        // segment looks only at its own lots.
        let mut fronting: HashMap<SegmentId, Vec<(LotId, &Lot)>> = HashMap::new();
        for (id, l) in lots.iter() {
            if !l.landlocked {
                fronting.entry(l.front_segment).or_default().push((id, l));
            }
        }
        for (sid, seg) in roads.alive_segments() {
            let (a, b) = roads.segment_points(sid);
            let len = a.distance(b).max(1e-3);
            let d = (b - a) / len;
            let right = Vec2::new(-d.y, d.x);
            let fronts = fronting.get(&sid).map(Vec::as_slice).unwrap_or(&[]);
            match seg.kind {
                RoadKind::Street => {
                    let off = seg.half_width() + SIDEWALK * 0.55;
                    for side in [-1i8, 1] {
                        let o = right * off * side as f32;
                        // Lots fronting this side, ordered along the segment.
                        let mut stops: Vec<(f32, LotId)> = fronts
                            .iter()
                            .filter(|(_, l)| ((l.entrance() - a).dot(right) >= 0.0) == (side > 0))
                            .map(|(id, l)| (l.front_t.clamp(0.02, 0.98), *id))
                            .collect();
                        stops.sort_by(|x, y| x.0.total_cmp(&y.0));
                        let na = g.add_node(a + o, None);
                        let mut prev = na;
                        for (t, lid) in stops {
                            let n = g.add_node(a + d * (t * len) + o, None);
                            g.nodes[n as usize].near.push(lid);
                            g.link(prev, n, false);
                            front_nodes.insert(lid, n);
                            street_nodes.push(n);
                            prev = n;
                        }
                        let nb = g.add_node(b + o, None);
                        g.link(prev, nb, false);
                        ends[seg.a.0 as usize].push((na, sid, side));
                        ends[seg.b.0 as usize].push((nb, sid, side));
                        street_nodes.push(na);
                        street_nodes.push(nb);
                    }
                }
                RoadKind::Rail => {}
                RoadKind::Path => {
                    // Homes may line a path: a node opposite each of them.
                    let mut stops: Vec<(f32, LotId)> = fronts.iter().map(|(id, l)| (l.front_t.clamp(0.02, 0.98), *id)).collect();
                    stops.sort_by(|x, y| x.0.total_cmp(&y.0));
                    let na = g.add_node(a, None);
                    let mut prev = na;
                    for (t, lid) in stops {
                        let n = g.add_node(a + d * (t * len), None);
                        g.nodes[n as usize].near.push(lid);
                        g.link(prev, n, false);
                        front_nodes.insert(lid, n);
                        street_nodes.push(n);
                        prev = n;
                    }
                    let nb = g.add_node(b, None);
                    g.link(prev, nb, false);
                    ends[seg.a.0 as usize].push((na, sid, 0));
                    ends[seg.b.0 as usize].push((nb, sid, 0));
                    street_nodes.push(na);
                    street_nodes.push(nb);
                }
            }
        }
        // Joins at road nodes: plain bends link matching sides directly;
        // junctions and dead ends get a crossing hub at the node.
        for (ni, node) in roads.nodes.iter().enumerate() {
            let e = &ends[ni];
            if e.is_empty() {
                continue;
            }
            let degree = node.segments.iter().filter(|&&s| roads.segment(s).alive).count();
            if degree == 2 && e.len() >= 2 {
                // Pair each endpoint of one segment with the nearest of the other.
                let first_seg = e[0].1;
                let (mine, theirs): (Vec<_>, Vec<_>) = e.iter().partition(|x| x.1 == first_seg);
                for (na, _, _) in &mine {
                    if let Some((nb, _, _)) = theirs.iter().min_by(|x, y| g.nodes[x.0 as usize].pos.distance(g.nodes[*na as usize].pos).total_cmp(&g.nodes[y.0 as usize].pos.distance(g.nodes[*na as usize].pos))) {
                        g.link(*na, *nb, false);
                    }
                }
                if theirs.is_empty() {
                    let hub = g.add_node(node.pos, None);
                    for (n, _, _) in e {
                        g.link(*n, hub, false);
                    }
                }
            } else {
                let hub = g.add_node(node.pos, None);
                for (n, _, _) in e {
                    g.link(*n, hub, false);
                }
            }
        }

        // ── lots ─────────────────────────────────────────────────────────
        // Each usable lot gets a loop inset from its edges with a node at
        // every vertex and every edge midpoint; the front midpoint links to
        // the nearest street node.
        let street_grid = Grid::new(NODE_CELL, street_nodes.iter().map(|&n| (g.nodes[n as usize].pos, g.nodes[n as usize].pos)));
        struct LoopInfo {
            lot: LotId,
            /// Node at each edge midpoint, by edge index.
            mids: Vec<u32>,
            through: bool,
        }
        let mut loops: Vec<LoopInfo> = Vec::new();
        for (lid, lot) in lots.iter() {
            if lot.polygon.len() < 3 {
                continue;
            }
            let ring = geom::inset_ring(&lot.polygon, LOOP_INSET);
            if ring.len() != lot.polygon.len() {
                continue;
            }
            let n = ring.len();
            let verts: Vec<u32> = ring.iter().map(|p| g.add_node(*p, Some(lid))).collect();
            let mut mids = Vec::with_capacity(n);
            for i in 0..n {
                let m = g.add_node(ring[i].lerp(ring[(i + 1) % n], 0.5), Some(lid));
                g.link(verts[i], m, true);
                g.link(m, verts[(i + 1) % n], true);
                mids.push(m);
            }
            // Front edge midpoint to the pavement.
            let front_i = (0..n)
                .min_by(|&a, &b| {
                    let ma = (lot.polygon[a] + lot.polygon[(a + 1) % n]) * 0.5;
                    let mb = (lot.polygon[b] + lot.polygon[(b + 1) % n]) * 0.5;
                    ma.distance(lot.entrance()).total_cmp(&mb.distance(lot.entrance()))
                })
                .unwrap_or(0);
            let front = mids[front_i];
            let fp = g.nodes[front as usize].pos;
            match front_nodes.get(&lid) {
                Some(&n) => g.link(front, n, true),
                None => {
                    // Items are positions in `street_nodes`, so a tie goes
                    // to the earlier entry as a scan would resolve it.
                    let within = |i: u32| {
                        let d = g.nodes[street_nodes[i as usize] as usize].pos.distance(fp);
                        if d < FRONT_REACH { d } else { f32::INFINITY }
                    };
                    if let Some((i, _)) = street_grid.nearest(fp, within) {
                        let best = street_nodes[i as usize];
                        g.link(front, best, true);
                        g.nodes[best as usize].near.push(lid);
                    }
                }
            }
            // The Stacks get an alley cross through the middle, and so does
            // unusable ground: it is open land people cross freely.
            if lot.landlocked || (lot.land_use == LandUse::Residential && Tier::for_land_value(land_value.at(lot.center)) == Tier::Low) {
                let c = g.add_node(lot.center, Some(lid));
                let mut picks: Vec<(f32, u32)> = mids.iter().map(|&m| (g.nodes[m as usize].pos.distance(lot.center), m)).collect();
                picks.sort_by(|a, b| a.0.total_cmp(&b.0));
                for (_, m) in picks.into_iter().take(4) {
                    g.link(c, m, true);
                }
            }
            loops.push(LoopInfo { lot: lid, mids, through: walkable_through(lot, land_value) });
        }

        // ── gates between neighbouring lots ──────────────────────────────
        // Edges that run along each other for a few metres get a gate at the
        // shared stretch, linking the two loops' midpoint nodes.
        let mut grid: HashMap<(i32, i32), Vec<(usize, usize)>> = HashMap::new();
        let cell = 12.0;
        for (li, info) in loops.iter().enumerate() {
            let poly = &lots[info.lot].polygon;
            let n = poly.len();
            for i in 0..n {
                let m = (poly[i] + poly[(i + 1) % n]) * 0.5;
                grid.entry(((m.x / cell).floor() as i32, (m.y / cell).floor() as i32)).or_default().push((li, i));
            }
        }
        let mut gated: Vec<(u32, u32)> = Vec::new();
        for (li, info) in loops.iter().enumerate() {
            if !info.through {
                continue;
            }
            let poly = &lots[info.lot].polygon;
            let n = poly.len();
            for i in 0..n {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                let m = (a + b) * 0.5;
                let cx = (m.x / cell).floor() as i32;
                let cy = (m.y / cell).floor() as i32;
                for dx in -1..=1 {
                    for dy in -1..=1 {
                        let Some(list) = grid.get(&(cx + dx, cy + dy)) else { continue };
                        for &(lj, j) in list {
                            if lj <= li || !loops[lj].through {
                                continue;
                            }
                            let other = &lots[loops[lj].lot].polygon;
                            let (c, d) = (other[j], other[(j + 1) % other.len()]);
                            if let Some((lo, hi)) = geom::shared_stretch(a, b, c, d) {
                                if hi - lo >= GATE_MIN_SHARED {
                                    let na = info.mids[i];
                                    let nb = loops[lj].mids[j];
                                    if !gated.contains(&(na, nb)) {
                                        gated.push((na, nb));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        for (a, b) in gated {
            g.link(a, b, true);
        }

        // ── parks ────────────────────────────────────────────────────────
        // Park paths are public: every path vertex is a node, each path's
        // first point (an entrance) joins the lot loop, and benches hang
        // off the nearest path node so people can walk to them and sit.
        let loop_mids: HashMap<LotId, Vec<u32>> = loops.iter().map(|l| (l.lot, l.mids.clone())).collect();
        for (lid, lot) in lots.iter() {
            let Some(park) = &lot.park else { continue };
            let mut path_nodes: Vec<u32> = Vec::new();
            for path in &park.paths {
                let mut prev: Option<u32> = None;
                for p in path {
                    // Reuse a node already at this spot (the hub is shared).
                    let n = match path_nodes.iter().copied().find(|&n| g.nodes[n as usize].pos.distance(*p) < 0.3) {
                        Some(n) => n,
                        None => {
                            let n = g.add_node(*p, Some(lid));
                            path_nodes.push(n);
                            n
                        }
                    };
                    if let Some(pv) = prev {
                        g.link_kind(pv, n, true, true);
                    }
                    prev = Some(n);
                }
                if let (Some(&first), Some(mids)) = (path.first(), loop_mids.get(&lid)) {
                    let entrance = path_nodes.iter().copied().find(|&n| g.nodes[n as usize].pos.distance(first) < 0.3);
                    let nearest = mids.iter().copied().min_by(|&a, &b| g.nodes[a as usize].pos.distance(first).total_cmp(&g.nodes[b as usize].pos.distance(first)));
                    if let (Some(e), Some(m)) = (entrance, nearest) {
                        g.link_kind(e, m, true, true);
                    }
                }
            }
            for (pos, facing) in &park.benches {
                let n = g.add_node(*pos, Some(lid));
                if let Some(&near) = path_nodes.iter().min_by(|&&a, &&b| g.nodes[a as usize].pos.distance(*pos).total_cmp(&g.nodes[b as usize].pos.distance(*pos))) {
                    g.link_kind(n, near, true, true);
                }
                g.benches.push((n, lid, *facing));
            }
        }
        g.index = Grid::new(NODE_CELL, g.nodes.iter().map(|n| (n.pos, n.pos)));
        g
    }

    /// A bench in a lot, as a position and facing; `pick` chooses which.
    pub fn bench_in(&self, lot: LotId, pick: usize) -> Option<(Vec2, f32)> {
        let here: Vec<(Vec2, f32)> = self.benches.iter().filter(|(_, l, _)| *l == lot).map(|(n, _, f)| (self.nodes[*n as usize].pos, *f)).collect();
        if here.is_empty() {
            None
        } else {
            Some(here[pick % here.len()])
        }
    }

    /// Nearest node within `max_dist`; exact ties go to the lower index.
    pub fn nearest(&self, p: Vec2, max_dist: f32) -> Option<u32> {
        let within = |i: usize| {
            let d = self.nodes[i].pos.distance(p);
            if d <= max_dist { d } else { f32::INFINITY }
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

    /// A* between two nodes with the edge costs of [`PARK_FACTOR`] and
    /// [`INTERIOR_FACTOR`]. The heuristic is the straight-line distance
    /// scaled by the cheapest factor, so it never overestimates and the
    /// first time the goal pops it is optimal.
    pub fn route_nodes(&self, from: u32, to: u32) -> Option<Vec<u32>> {
        #[derive(PartialEq)]
        struct Item(f32, u32);
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
        heap.push(Item(h(from), from));
        while let Some(Item(_, u)) = heap.pop() {
            if u == to {
                break;
            }
            let du = dist[u as usize];
            for e in &self.adj[u as usize] {
                let cost = e.len * if e.park { PARK_FACTOR } else if e.interior { INTERIOR_FACTOR } else { 1.0 };
                let nd = du + cost;
                if nd < dist[e.to as usize] {
                    dist[e.to as usize] = nd;
                    prev[e.to as usize] = u;
                    heap.push(Item(nd + h(e.to), e.to));
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
        let a = self.nearest(from, 60.0)?;
        let b = self.nearest(to, 60.0)?;
        let nodes = self.route_nodes(a, b)?;
        let pts = nodes.iter().map(|&i| self.nodes[i as usize].pos).collect();
        Some((pts, nodes))
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

    /// Interior edges (alleys, loops, gates) for drawing.
    pub fn interior_edges(&self) -> Vec<(Vec2, Vec2)> {
        let mut out = Vec::new();
        for (i, edges) in self.adj.iter().enumerate() {
            for e in edges {
                if e.interior && (i as u32) < e.to {
                    out.push((self.nodes[i].pos, self.nodes[e.to as usize].pos));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zoning::{generate_lots, Zone, ZoneId};

    /// A hand-built graph: `nodes` positions and `edges` as (a, b, interior, park).
    fn graph(nodes: &[Vec2], edges: &[(u32, u32, bool, bool)]) -> WalkGraph {
        let mut g = WalkGraph::default();
        for p in nodes {
            g.add_node(*p, None);
        }
        for &(a, b, interior, park) in edges {
            g.link_kind(a, b, interior, park);
        }
        g.index = Grid::new(NODE_CELL, g.nodes.iter().map(|n| (n.pos, n.pos)));
        g
    }

    #[test]
    fn the_heuristic_never_talks_a_route_out_of_a_cheaper_park_path() {
        // Start and goal 100 m apart on a pavement; a park path makes a
        // detour of 108 m that costs 99.36 with the park factor. With the
        // plain straight-line heuristic the goal popped first (f = 100
        // against 103.68 for the detour's corner) and the dearer pavement
        // was returned.
        let s = Vec2::ZERO;
        let goal = Vec2::new(100.0, 0.0);
        let corner = Vec2::new(50.0, (54.0f32 * 54.0 - 50.0 * 50.0).sqrt());
        let g = graph(&[s, goal, corner], &[(0, 1, false, false), (0, 2, true, true), (2, 1, true, true)]);
        let route = g.route_nodes(0, 1).expect("route");
        assert_eq!(route, vec![0, 2, 1], "the park detour is cheaper: 0.92 x 108 < 100");
        // And the pavement wins once the detour costs more than it saves.
        let far = Vec2::new(50.0, 40.0);
        let g = graph(&[s, goal, far], &[(0, 1, false, false), (0, 2, true, true), (2, 1, true, true)]);
        assert_eq!(g.route_nodes(0, 1).unwrap(), vec![0, 1]);
    }

    #[test]
    fn nearest_node_matches_a_scan_and_respects_the_radius() {
        let pts: Vec<Vec2> = (0..15).flat_map(|y| (0..15).map(move |x| Vec2::new(x as f32 * 9.0, y as f32 * 9.0))).collect();
        let g = graph(&pts, &[]);
        for q in [Vec2::new(4.5, 4.5), Vec2::new(31.0, 70.2), Vec2::new(-3.0, 200.0), Vec2::new(63.0, 63.0)] {
            let lin = (0..pts.len()).filter(|&i| pts[i].distance(q) <= 60.0).min_by(|&a, &b| pts[a].distance(q).total_cmp(&pts[b].distance(q))).map(|i| i as u32);
            assert_eq!(g.nearest(q, 60.0), lin, "at {q:?}");
        }
        assert!(g.nearest(Vec2::new(500.0, 500.0), 60.0).is_none());
        assert!(WalkGraph::default().nearest(Vec2::ZERO, 60.0).is_none());
    }

    #[test]
    fn walkers_cut_through_blocks_and_shops_see_them() {
        let mut roads = RoadNetwork::default();
        for c in [-70.0f32, 70.0] {
            roads.add_polyline(&[Vec2::new(-300.0, c), Vec2::new(300.0, c)], 1, 1);
            roads.add_polyline(&[Vec2::new(c, -300.0), Vec2::new(c, 300.0)], 1, 1);
        }
        let zone = Zone {
            polygon: vec![Vec2::new(-65.0, -65.0), Vec2::new(65.0, -65.0), Vec2::new(65.0, 65.0), Vec2::new(-65.0, 65.0)],
            land_use: LandUse::Commercial,
        };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        let lv = LandValue::new(400.0, 32.0);
        let g = WalkGraph::build(&roads, &lots, &lv, 1);
        assert!(g.nodes.len() > 50);
        assert!(g.adj.iter().any(|e| e.iter().any(|x| x.interior)), "lots have interior paths");
        // Crossing the block from the south pavement to the north one goes
        // through the lots instead of around the outside.
        let from = Vec2::new(0.0, -62.0);
        let to = Vec2::new(0.0, 62.0);
        let (pts, nodes) = g.route(from, to).expect("route exists");
        let walked: f32 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
        assert!(walked < 180.0, "cut through the block: {walked} (around would be about 244)");
        assert!(nodes.windows(2).any(|w| g.adj[w[0] as usize].iter().any(|e| e.to == w[1] && e.interior)), "uses a lot interior");
        let passed = g.lots_passed(&nodes);
        assert!(!passed.is_empty(), "the route passes lots");
    }

    #[test]
    fn dump_walk_graph_for_inspection() {
        let Ok(path) = std::env::var("ND_WALK_DUMP") else { return };
        let mut roads = RoadNetwork::default();
        for c in [-70.0f32, 70.0] {
            roads.add_polyline(&[Vec2::new(-300.0, c), Vec2::new(300.0, c)], 1, 1);
            roads.add_polyline(&[Vec2::new(c, -300.0), Vec2::new(c, 300.0)], 1, 1);
        }
        let zone = Zone {
            polygon: vec![Vec2::new(-65.0, -65.0), Vec2::new(65.0, -65.0), Vec2::new(65.0, 65.0), Vec2::new(-65.0, 65.0)],
            land_use: LandUse::Commercial,
        };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        let lv = LandValue::new(400.0, 32.0);
        let g = WalkGraph::build(&roads, &lots, &lv, 1);
        let mut out = String::new();
        for l in lots.values() {
            let pts: Vec<String> = l.polygon.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
            out.push_str(&format!("{}|{}
", if l.usable() { "lot" } else { "dead" }, pts.join(";")));
        }
        for (i, edges) in g.adj.iter().enumerate() {
            for e in edges {
                if (i as u32) < e.to {
                    let a = g.nodes[i].pos;
                    let b = g.nodes[e.to as usize].pos;
                    out.push_str(&format!("{}|{},{};{},{}
", if e.interior { "walk_in" } else { "walk" }, a.x, a.y, b.x, b.y));
                }
            }
        }
        if let Some((pts, _)) = g.route(Vec2::new(0.0, -62.0), Vec2::new(0.0, 62.0)) {
            let s: Vec<String> = pts.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
            out.push_str(&format!("route|{}
", s.join(";")));
        }
        let _ = std::fs::write(path, out);
    }
}
