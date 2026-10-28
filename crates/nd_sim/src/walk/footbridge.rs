//! Footbridges: a crossing raised over the road, its stairs and the levels
//! walkers climb to.

use super::*;

/// [tools] A footbridge over a crossing: its walkway from `top[0]` to
/// `top[1]` at `level` above the street, and the stairs down from each end
/// to `foot[0]` and `foot[1]` on the pavements.
#[derive(Clone, Debug)]
pub struct Footbridge {
    pub crossing: u32,
    pub top: [Vec2; 2],
    pub foot: [Vec2; 2],
    pub level: f32,
    pub width: f32,
    /// Walk nodes: the two feet and the two tops (a joined end's "foot"
    /// is its top).
    pub nodes: [u32; 4],
    /// [pt-walk] An end that meets another footbridge's end at a corner
    /// joins its deck (`WalkGraph::footbridge_links`) instead of having
    /// stairs of its own: footbridges round a junction make a ring.
    pub joined: [bool; 2],
    /// [r1-roads] (K14) A raised path coming down to the street at the
    /// crossing's node joins the bridge instead: (the end it joins, the
    /// path's last raised node, the node at the street). The path stays up
    /// on its deck to the bridge (`nd_procgen::terrain` draws it so), its
    /// ramp down to the street is not walked, and that end has no stairs.
    pub path_join: Option<(usize, NodeId, NodeId)>,
}

/// [r1-roads] (K14) The raised path coming down onto the street at road
/// node `n`: the path piece from `n` whose other node is up on a deck
/// (`VIADUCT_LEVEL` or more), as (that node, the piece).
pub fn raised_path_down_to(roads: &RoadNetwork, n: NodeId) -> Option<(NodeId, SegmentId)> {
    roads.node(n).segments.iter().copied().filter(|&s| roads.try_segment(s).is_some_and(|sg| sg.kind == RoadKind::Path)).find_map(|s| {
        let sg = roads.segment(s);
        let far = if sg.a == n { sg.b } else { sg.a };
        (roads.node(far).level >= crate::roads::VIADUCT_LEVEL && roads.node(n).level < crate::roads::VIADUCT_LEVEL).then_some((far, s))
    })
}

/// [pt-walk] Two footbridge ends at most this far apart (at a junction's
/// corner) are joined by a corner deck.
pub const CORNER_JOIN: f32 = 9.0;

impl WalkGraph {
    /// [tools] Where a footbridge over crossing `k` would stand: its
    /// walkway's ends and the stairs' feet (`add_footbridge`).
    pub fn footbridge_shape(&self, k: u32, roads: &RoadNetwork) -> Option<([Vec2; 2], [Vec2; 2])> {
        use crate::junctions::{FOOTBRIDGE_RUN, FOOTBRIDGE_WIDTH};
        let c = self.crossings.get(k as usize)?;
        roads.try_segment(c.segment)?;
        let along = self.stair_dirs(c, roads);
        let top = [self.nodes[c.a as usize].pos + along[0] * (FOOTBRIDGE_WIDTH + 0.5), self.nodes[c.b as usize].pos + along[1] * (FOOTBRIDGE_WIDTH + 0.5)];
        Some((top, [top[0] + along[0] * FOOTBRIDGE_RUN, top[1] + along[1] * FOOTBRIDGE_RUN]))
    }

    /// The way a footbridge's stairs run: along the street away from the
    /// junction (or either way mid-block).
    pub(super) fn stair_dir(c: &Crossing, dir: Vec2, roads: &RoadNetwork) -> Vec2 {
        match c.node {
            Some(n) if (c.mid - roads.node(n).pos).dot(dir) < 0.0 => -dir,
            _ => dir,
        }
    }

    /// [walk-crossings] The way each end's stairs run. Over a street, both
    /// along it away from the junction (`stair_dir`). Over a slip road's
    /// mouth, which has no pavement of its own, each end's stairs run down
    /// the pavement of the street that end stands on, away from the
    /// junction, and the deck spans the mouth from corner to corner.
    pub(super) fn stair_dirs(&self, c: &Crossing, roads: &RoadNetwork) -> [Vec2; 2] {
        let (s0, s1) = roads.segment_points(c.segment);
        let dir = (s1 - s0).normalize_or_zero();
        let along = Self::stair_dir(c, dir, roads);
        let Some(n) = c.node.filter(|_| c.ramp) else { return [along; 2] };
        let centre = roads.node(n).pos;
        let end_dir = |p: Vec2| {
            // The street whose pavement line the end lies on.
            roads
                .node(n)
                .segments
                .iter()
                .copied()
                .filter(|&s| {
                    let sg = roads.segment(s);
                    sg.alive && sg.is_street() && !sg.fronts_nothing()
                })
                .map(|s| {
                    let away = roads.away_dir(s, n);
                    let off = (p - centre).perp_dot(away).abs();
                    let want = roads.segment(s).half_width() + crate::zoning::SIDEWALK * 0.55;
                    ((off - want).abs() + if (p - centre).dot(away) < 0.0 { 50.0 } else { 0.0 }, s, away)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
                .map_or(along, |(_, _, away)| away)
        };
        [end_dir(self.nodes[c.a as usize].pos), end_dir(self.nodes[c.b as usize].pos)]
    }

    /// A footbridge over crossing `k`: the walkway a little to one side of
    /// the crosswalk at `FOOTBRIDGE_LEVEL`, stairs from each end along the
    /// pavement (`along`), and the feet joined to the pavement both ways.
    pub(super) fn add_footbridge(&mut self, k: u32, along: [Vec2; 2], roads: &RoadNetwork) {
        use crate::junctions::{FOOTBRIDGE_LEVEL, FOOTBRIDGE_RUN, FOOTBRIDGE_WIDTH};
        let c = self.crossings[k as usize].clone();
        let ends = [c.a, c.b];
        let mut nodes = [0u32; 4];
        let mut tops = [Vec2::ZERO; 2];
        let mut feet = [Vec2::ZERO; 2];
        let mut joined = [false; 2];
        // [r1-roads] (K14) A raised path coming down to the street right
        // here (the crossing is its zebra): the end on its side joins it.
        let mut path_join: Option<(usize, NodeId, NodeId)> = None;
        if let Some(n) = c.node.filter(|_| c.kind == CrossingKind::Zebra) {
            if let Some((r, _)) = raised_path_down_to(roads, n) {
                let rp = roads.node(r).pos;
                let side = |e: u32| (self.nodes[e as usize].pos - roads.node(n).pos).dot(rp - roads.node(n).pos);
                let i = if side(c.a) >= side(c.b) { 0 } else { 1 };
                path_join = Some((i, r, n));
            }
        }
        for (i, &end) in ends.iter().enumerate() {
            let along = along[i];
            let p = self.nodes[end as usize].pos;
            let top = p + along * (FOOTBRIDGE_WIDTH + 0.5);
            let foot = top + along * FOOTBRIDGE_RUN;
            let t = self.add_node(top, None);
            self.nodes[t as usize].level = FOOTBRIDGE_LEVEL;
            if let Some((_, r, n)) = path_join.filter(|j| j.0 == i) {
                // The deck goes on to the path's deck; the path's ramp down
                // to the street is not walked (nor drawn) any more.
                let (rp, np) = (roads.node(r).pos, roads.node(n).pos);
                let on_ramp = |q: Vec2| crate::geom::project_on_segment(q, rp, np).1.distance(q) < 0.3;
                let ramp: Vec<u32> = (0..self.nodes.len() as u32).filter(|&m| m != t && self.nodes[m as usize].level == 0.0 && on_ramp(self.nodes[m as usize].pos)).collect();
                for &m in &ramp {
                    let pm = self.nodes[m as usize].pos;
                    self.adj[m as usize].retain(|e| !(ramp.contains(&e.to) && pm.distance(rp) > 0.3) && !(ramp.contains(&e.to) && self.nodes[e.to as usize].pos.distance(rp) > 0.3));
                }
                if let Some(&at_r) = ramp.iter().find(|&&m| self.nodes[m as usize].pos.distance(rp) < 0.3) {
                    self.link_raised(t, at_r);
                }
                joined[i] = true;
                nodes[i * 2] = t;
                nodes[i * 2 + 1] = t;
                tops[i] = top;
                feet[i] = top;
                continue;
            }
            // [pt-walk] Another footbridge's end at this corner: a corner
            // deck over to it, and no stairs of our own here.
            let near = self
                .footbridges
                .iter()
                .flat_map(|f| [(f.top[0], f.nodes[2]), (f.top[1], f.nodes[3])])
                .map(|(q, n)| (q.distance(top), q, n))
                .filter(|(d, _, _)| *d < CORNER_JOIN)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((_, q, n)) = near {
                self.link_raised(t, n);
                self.footbridge_links.push([q, top]);
                joined[i] = true;
                nodes[i * 2] = t;
                nodes[i * 2 + 1] = t;
                tops[i] = top;
                feet[i] = top;
                continue;
            }
            let f = self.add_node(foot, None);
            // The stairs.
            self.link_raised(f, t);
            // The foot on the pavement: back to the crosswalk's end, and on
            // to the next pavement node beyond it, so nobody doubles back.
            self.link(f, end, false);
            let beyond = self.adj[end as usize]
                .iter()
                .filter(|e| e.cross == NO_CROSSING && !e.raised && e.to != f)
                .map(|e| (e.to, (self.nodes[e.to as usize].pos - p).dot(along), self.nodes[e.to as usize].pos.distance(p)))
                .filter(|&(_, d, _)| d > FOOTBRIDGE_RUN * 0.5)
                .min_by(|a, b| a.2.total_cmp(&b.2));
            if let Some((q, _, _)) = beyond {
                self.link(f, q, false);
            }
            nodes[i * 2] = f;
            nodes[i * 2 + 1] = t;
            tops[i] = top;
            feet[i] = foot;
        }
        // The walkway.
        self.link_raised(nodes[1], nodes[3]);
        self.crossings[k as usize].bridged = true;
        self.footbridges.push(Footbridge { crossing: k, top: tops, foot: feet, level: FOOTBRIDGE_LEVEL, width: FOOTBRIDGE_WIDTH, nodes: [nodes[0], nodes[2], nodes[1], nodes[3]], joined, path_join });
    }

    /// [footbridge] Take crossing `k`'s at-grade edge out of the walk graph
    /// (its footbridge stands in for it). The crossing itself stays listed,
    /// `closed`, so indices and the editor's view of it hold.
    pub(super) fn close_crossing(&mut self, k: u32) {
        let Some(c) = self.crossings.get_mut(k as usize) else { return };
        c.closed = true;
        let (a, b) = (c.a as usize, c.b as usize);
        self.adj[a].retain(|e| e.cross != k);
        self.adj[b].retain(|e| e.cross != k);
    }

    /// Join two nodes by a raised walkway edge (a deck or a flight of
    /// stairs), its length the walk along the slope.
    pub(super) fn link_raised(&mut self, a: u32, b: u32) {
        if a == b {
            return;
        }
        let (na, nb) = (&self.nodes[a as usize], &self.nodes[b as usize]);
        let len = (na.pos.distance(nb.pos).powi(2) + (na.level - nb.level).powi(2)).sqrt().max(0.05);
        let (lo, hi) = if na.level <= nb.level { (a, b) } else { (b, a) };
        self.adj[a as usize].push(WalkEdge { to: b, len, interior: false, park: false, cross: NO_CROSSING, raised: true, gate: false });
        self.adj[b as usize].push(WalkEdge { to: a, len, interior: false, park: false, cross: NO_CROSSING, raised: true, gate: false });
        self.raised.push((lo, hi));
    }

    /// [walk-crossings] The crossing whose footbridge's deck runs from `a`
    /// to `b` (its two tops, either way round, exactly), if any.
    pub fn footbridge_deck(&self, a: Vec2, b: Vec2) -> Option<u32> {
        if self.level_at_node(a) == 0.0 || self.level_at_node(b) == 0.0 {
            return None;
        }
        self.footbridges.iter().find(|f| (f.top[0] == a && f.top[1] == b) || (f.top[0] == b && f.top[1] == a)).map(|f| f.crossing)
    }

    /// [r4-crossings] Mark the at-grade crossings within `BRIDGE_DRAW` of
    /// a way over the same street off the road (a footbridge, or a path
    /// passing under or over it): walkers pay `BRIDGED_GRADE_PENALTY` to
    /// use them, so they take the bridge or the underpass when it is a
    /// short way round. Run after the footbridges are up.
    pub fn mark_near_bridges(&mut self, roads: &RoadNetwork) {
        let dir_of = |sid: SegmentId| {
            let (s0, s1) = roads.segment_points(sid);
            (s1 - s0).normalize_or_zero()
        };
        // (where it crosses, the street's direction there)
        let mut over: Vec<(Vec2, Vec2)> = Vec::new();
        for f in &self.footbridges {
            let c = &self.crossings[f.crossing as usize];
            if roads.try_segment(c.segment).is_some() {
                over.push((c.mid, dir_of(c.segment)));
            }
        }
        for x in roads.grade_crossings() {
            let (u, l) = (roads.segment(x.upper.0), roads.segment(x.lower.0));
            let street = match (u.kind, l.kind) {
                (RoadKind::Path, RoadKind::Street) if l.walkable() => x.lower.0,
                (RoadKind::Street, RoadKind::Path) if u.walkable() => x.upper.0,
                _ => continue,
            };
            over.push((x.at, dir_of(street)));
        }
        if over.is_empty() {
            return;
        }
        for c in &mut self.crossings {
            if c.closed || c.bridged || roads.try_segment(c.segment).is_none() {
                continue;
            }
            let d = dir_of(c.segment);
            c.near_bridge = over.iter().any(|&(at, od)| d.dot(od).abs() > 0.8 && c.mid.distance(at) <= BRIDGE_DRAW && (c.mid - at).perp_dot(od).abs() < 6.0);
        }
    }

    /// [tools] Height above the ground of a walk node at exactly `p` (a
    /// point of a walker's path): 0 off the raised walkways.
    pub fn level_at_node(&self, p: Vec2) -> f32 {
        if self.raised_at.is_empty() {
            return 0.0;
        }
        self.raised_at.get(&(p.x.to_bits(), p.y.to_bits())).copied().unwrap_or(0.0)
    }

    /// [tools] How high a walker at `pos` stands on the leg of its path
    /// from `a` to `b` (node positions): along the stairs, up on the deck.
    pub fn level_on_leg(&self, a: Vec2, b: Vec2, pos: Vec2) -> f32 {
        let (la, lb) = (self.level_at_node(a), self.level_at_node(b));
        if la == 0.0 && lb == 0.0 {
            return 0.0;
        }
        let (t, _) = geom::project_on_segment(pos, a, b);
        la + (lb - la) * t
    }
}
