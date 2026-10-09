//! Node-graph pathfinding (A*) over the road network, and its leaves.

use super::*;

impl RoadNetwork {
    pub fn neighbors(&self, n: NodeId) -> impl Iterator<Item = (NodeId, SegmentId)> + '_ {
        self.node(n).segments.iter().filter_map(move |&sid| {
            let s = self.segment(sid);
            if !s.alive {
                return None;
            }
            Some((if s.a == n { s.b } else { s.a }, sid))
        })
    }

    /// Nodes with a single street or path: the city's entrances and dead ends.
    pub fn leaf_nodes(&self) -> Vec<NodeId> {
        self.leaf_nodes_where(|s| !s.is_rail())
    }

    /// Ends of the railway: where trains enter and leave.
    pub fn rail_leaf_nodes(&self) -> Vec<NodeId> {
        self.leaf_nodes_where(|s| s.is_rail())
    }

    pub(super) fn leaf_nodes_where(&self, keep: impl Fn(&Segment) -> bool) -> Vec<NodeId> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| {
                let mine: Vec<_> = n.segments.iter().filter(|&&s| self.segment(s).alive && keep(self.segment(s))).collect();
                mine.len() == 1 && n.segments.iter().filter(|&&s| self.segment(s).alive).count() == 1
            })
            .map(|(i, _)| NodeId(i as u32))
            .collect()
    }

    /// A* over street and path nodes. Returns the node sequence including both ends.
    pub fn path(&self, from: NodeId, to: NodeId) -> Option<Vec<NodeId>> {
        self.path_where(from, to, |s| !s.is_rail())
    }

    /// A* for walkers and bikes: streets and paths, but never a tunnel, a
    /// highway or a slip road (`Segment::fronts_nothing`). A rider routed
    /// through a bore was drawn on the ground above it.
    pub fn foot_path(&self, from: NodeId, to: NodeId) -> Option<Vec<NodeId>> {
        self.path_where(from, to, |s| !s.is_rail() && !s.fronts_nothing())
    }

    /// A* along the railway only.
    pub fn rail_path(&self, from: NodeId, to: NodeId) -> Option<Vec<NodeId>> {
        self.path_where(from, to, |s| s.is_rail())
    }

    pub fn path_where(&self, from: NodeId, to: NodeId, keep: impl Fn(&Segment) -> bool) -> Option<Vec<NodeId>> {
        if from == to {
            return Some(vec![from]);
        }
        let goal = self.node(to).pos;
        let mut open = BinaryHeap::new();
        let mut came_from: HashMap<NodeId, NodeId> = HashMap::new();
        let mut g: HashMap<NodeId, f32> = HashMap::new();
        g.insert(from, 0.0);
        open.push(Open { f: self.node(from).pos.distance(goal), g: 0.0, node: from });

        while let Some(Open { node, g: popped_g, .. }) = open.pop() {
            // An entry left behind when the node was reached more cheaply.
            if popped_g > g[&node] {
                continue;
            }
            if node == to {
                let mut path = vec![to];
                let mut cur = to;
                while let Some(&prev) = came_from.get(&cur) {
                    path.push(prev);
                    cur = prev;
                }
                path.reverse();
                return Some(path);
            }
            let gn = g[&node];
            for (next, sid) in self.neighbors(node) {
                if !keep(self.segment(sid)) {
                    continue;
                }
                let cost = gn + self.node(node).pos.distance(self.node(next).pos);
                if g.get(&next).map_or(true, |&old| cost < old) {
                    g.insert(next, cost);
                    came_from.insert(next, node);
                    open.push(Open { f: cost + self.node(next).pos.distance(goal), g: cost, node: next });
                }
            }
        }
        None
    }
}

#[derive(PartialEq)]
pub(super) struct Open {
    pub(super) f: f32,
    /// The cost to reach `node` when pushed (stale once it is reached for less).
    pub(super) g: f32,
    pub(super) node: NodeId,
}

impl Eq for Open {}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap; invert so the smallest f pops first.
        other.f.total_cmp(&self.f).then_with(|| other.node.cmp(&self.node))
    }
}
