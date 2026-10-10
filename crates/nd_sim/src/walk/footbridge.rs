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
        let (s0, s1) = roads.segment_points(c.segment);
        let dir = (s1 - s0).normalize_or_zero();
        let along = Self::stair_dir(c, dir, roads);
        let top = [self.nodes[c.a as usize].pos + along * (FOOTBRIDGE_WIDTH + 0.5), self.nodes[c.b as usize].pos + along * (FOOTBRIDGE_WIDTH + 0.5)];
        Some((top, [top[0] + along * FOOTBRIDGE_RUN, top[1] + along * FOOTBRIDGE_RUN]))
    }

    /// The way a footbridge's stairs run: along the street away from the
    /// junction (or either way mid-block).
    pub(super) fn stair_dir(c: &Crossing, dir: Vec2, roads: &RoadNetwork) -> Vec2 {
        match c.node {
            Some(n) if (c.mid - roads.node(n).pos).dot(dir) < 0.0 => -dir,
            _ => dir,
        }
    }

    /// A footbridge over crossing `k`: the walkway a little to one side of
    /// the crosswalk at `FOOTBRIDGE_LEVEL`, stairs from each end along the
    /// pavement (`along`), and the feet joined to the pavement both ways.
    pub(super) fn add_footbridge(&mut self, k: u32, along: Vec2) {
        use crate::junctions::{FOOTBRIDGE_LEVEL, FOOTBRIDGE_RUN, FOOTBRIDGE_WIDTH};
        let c = self.crossings[k as usize].clone();
        let ends = [c.a, c.b];
        let mut nodes = [0u32; 4];
        let mut tops = [Vec2::ZERO; 2];
        let mut feet = [Vec2::ZERO; 2];
        let mut joined = [false; 2];
        for (i, &end) in ends.iter().enumerate() {
            let p = self.nodes[end as usize].pos;
            let top = p + along * (FOOTBRIDGE_WIDTH + 0.5);
            let foot = top + along * FOOTBRIDGE_RUN;
            let t = self.add_node(top, None);
            self.nodes[t as usize].level = FOOTBRIDGE_LEVEL;
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
        self.footbridges.push(Footbridge { crossing: k, top: tops, foot: feet, level: FOOTBRIDGE_LEVEL, width: FOOTBRIDGE_WIDTH, nodes: [nodes[0], nodes[2], nodes[1], nodes[3]], joined });
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
