//! [walk-networks] The walk graph's separate networks: every walkable edge
//! (pavements, paths, crossings, footbridges, tunnels, park walks, lot
//! loops) by the connected piece of the walk graph it belongs to, for the
//! "Walk networks" map layer and the remote API's `walk_networks`.
//!
//! Each network keeps an id across rebuilds of the walk graph, so its
//! colour does not jump about while the player builds: a network takes the
//! id of the largest network it overlaps from before (two networks joined
//! by a new path keep the larger one's id and colour; a network split in
//! two leaves the id with the larger piece). Id 1 is the first main
//! network. Ids are carried by where the networks lie (a 20 m grid of
//! their nodes), so nothing about them needs saving: after a load the
//! largest network is id 1 again.

use super::*;
use std::collections::HashMap;

/// Cell of the grid that carries ids over from one walk graph to the next.
const CELL: f32 = 20.0;

/// The networks' colours (linear RGB, 0..1), by id: id 1 (the first main
/// network) a calm cyan, the rest picked to stand apart from it and from
/// each other.
pub const PALETTE: [[f32; 3]; 12] = [
    [0.10, 0.75, 0.95],
    [1.00, 0.12, 0.10],
    [1.00, 0.80, 0.05],
    [0.95, 0.15, 0.85],
    [1.00, 0.45, 0.05],
    [0.25, 1.00, 0.20],
    [0.50, 0.25, 1.00],
    [1.00, 0.55, 0.65],
    [0.70, 1.00, 0.10],
    [0.15, 0.30, 1.00],
    [0.60, 0.35, 0.10],
    [1.00, 1.00, 1.00],
];

/// The colour of network `id`.
pub fn network_color(id: u32) -> [f32; 3] {
    PALETTE[(id.max(1) as usize - 1) % PALETTE.len()]
}

/// One connected network of the walk graph.
#[derive(Clone, Debug, Default)]
pub struct WalkNetwork {
    /// Kept across rebuilds (see the module docs); 1 is the first main one.
    pub id: u32,
    /// Its label in `WalkGraph::comp` now.
    pub comp: u32,
    pub nodes: u32,
    /// Walkable edges, each counted once.
    pub edges: u32,
    /// Of those, down in a tunnel.
    pub underground: u32,
    /// Buildings on it (by their lot's walk nodes), the most people first.
    pub buildings: Vec<BuildingId>,
    pub residents: u32,
    pub workers: u32,
    pub lo: Vec2,
    pub hi: Vec2,
    /// What to call it: after its busiest building, else its street.
    pub name: String,
}

impl WalkNetwork {
    /// Its colour (linear RGB).
    pub fn color(&self) -> [f32; 3] {
        network_color(self.id)
    }
}

/// Every network of the current walk graph, the main one (most buildings,
/// then most edges) first.
#[derive(Clone, Debug, Default)]
pub struct WalkNetworks {
    /// The walk graph version these were found for.
    pub version: u64,
    pub list: Vec<WalkNetwork>,
    /// Index into `list` by component label.
    by_comp: HashMap<u32, usize>,
    /// Network id by grid cell, from the last time, to carry ids over.
    cells: HashMap<(i32, i32), u32>,
    next_id: u32,
}

impl WalkNetworks {
    /// The main network: the most buildings (then the most edges).
    pub fn main(&self) -> Option<&WalkNetwork> {
        self.list.first()
    }

    /// The network walk node `n` is on.
    pub fn of_node(&self, walk: &WalkGraph, n: u32) -> Option<&WalkNetwork> {
        walk.component(n).and_then(|c| self.by_comp.get(&c)).map(|&i| &self.list[i])
    }

    /// The network with component label `comp`.
    pub fn of_comp(&self, comp: u32) -> Option<&WalkNetwork> {
        self.by_comp.get(&comp).map(|&i| &self.list[i])
    }

    pub fn by_id(&self, id: u32) -> Option<&WalkNetwork> {
        self.list.iter().find(|n| n.id == id)
    }

    /// Label the networks of `walk` (with `comp` computed), carrying ids
    /// over from the last call. `lot_info` gives a lot's building with its
    /// residents and workers; `street_name` names the street nearest a point.
    pub fn update(&mut self, walk: &WalkGraph, lot_info: impl Fn(LotId) -> Option<(BuildingId, u32, u32, String)>, street_name: impl Fn(Vec2) -> Option<String>) {
        self.version = walk.version;
        let n = walk.nodes.len();
        if walk.comp.len() != n {
            self.list.clear();
            self.by_comp.clear();
            return;
        }
        let cell = |p: Vec2| ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32);
        let mut nets: Vec<WalkNetwork> = Vec::new();
        let mut index: HashMap<u32, usize> = HashMap::new();
        // Each lot once, on the network its first walk node is on.
        let mut lot_seen: HashMap<LotId, ()> = HashMap::new();
        let mut people: Vec<Vec<(u32, BuildingId, String)>> = Vec::new();
        for i in 0..n {
            if walk.adj[i].is_empty() {
                continue;
            }
            let c = walk.comp[i];
            let k = *index.entry(c).or_insert_with(|| {
                nets.push(WalkNetwork { comp: c, lo: Vec2::splat(f32::INFINITY), hi: Vec2::splat(f32::NEG_INFINITY), ..Default::default() });
                people.push(Vec::new());
                nets.len() - 1
            });
            let node = &walk.nodes[i];
            let net = &mut nets[k];
            net.nodes += 1;
            net.lo = net.lo.min(node.pos);
            net.hi = net.hi.max(node.pos);
            for e in &walk.adj[i] {
                if (i as u32) < e.to {
                    net.edges += 1;
                    if node.level < 0.0 || walk.nodes[e.to as usize].level < 0.0 {
                        net.underground += 1;
                    }
                }
            }
            for l in node.lot.iter().chain(node.near.iter()) {
                if lot_seen.insert(*l, ()).is_some() {
                    continue;
                }
                if let Some((b, residents, workers, name)) = lot_info(*l) {
                    net.residents += residents;
                    net.workers += workers;
                    people[k].push((residents + workers, b, name));
                }
            }
        }
        for (net, ps) in nets.iter_mut().zip(people.iter_mut()) {
            ps.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            net.buildings = ps.iter().map(|p| p.1).collect();
            net.name = match ps.first() {
                Some((_, _, name)) if !name.is_empty() => format!("around {name}"),
                _ => street_name((net.lo + net.hi) * 0.5).map_or_else(|| format!("near ({:.0}, {:.0})", (net.lo.x + net.hi.x) * 0.5, (net.lo.y + net.hi.y) * 0.5), |s| format!("along {s}")),
            };
        }
        nets.sort_by(|a, b| b.buildings.len().cmp(&a.buildings.len()).then(b.edges.cmp(&a.edges)).then(a.comp.cmp(&b.comp)));
        // Ids: each network, largest first, takes the id of the largest
        // network from before that it overlaps and nobody has taken yet.
        let old_rank: HashMap<u32, usize> = self.list.iter().enumerate().map(|(r, x)| (x.id, r)).collect();
        let mut votes: Vec<Vec<u32>> = vec![Vec::new(); nets.len()];
        if !self.cells.is_empty() {
            let at: HashMap<u32, usize> = nets.iter().enumerate().map(|(k, x)| (x.comp, k)).collect();
            for i in 0..n {
                if walk.adj[i].is_empty() {
                    continue;
                }
                if let (Some(&k), Some(&id)) = (at.get(&walk.comp[i]), self.cells.get(&cell(walk.nodes[i].pos))) {
                    if !votes[k].contains(&id) {
                        votes[k].push(id);
                    }
                }
            }
        }
        let mut taken: Vec<u32> = Vec::new();
        self.next_id = self.next_id.max(self.list.iter().map(|x| x.id).max().unwrap_or(0));
        for (k, net) in nets.iter_mut().enumerate() {
            let pick = votes[k].iter().copied().filter(|id| !taken.contains(id)).min_by_key(|id| old_rank.get(id).copied().unwrap_or(usize::MAX));
            net.id = match pick {
                Some(id) => id,
                None => {
                    self.next_id += 1;
                    self.next_id
                }
            };
            taken.push(net.id);
        }
        // The grid for next time: each cell to the largest network in it.
        self.cells.clear();
        let id_of: HashMap<u32, (u32, usize)> = nets.iter().enumerate().map(|(k, x)| (x.comp, (x.id, k))).collect();
        let mut rank_at: HashMap<(i32, i32), usize> = HashMap::new();
        for i in 0..n {
            if walk.adj[i].is_empty() {
                continue;
            }
            let Some(&(id, rank)) = id_of.get(&walk.comp[i]) else { continue };
            let c = cell(walk.nodes[i].pos);
            if rank_at.get(&c).is_none_or(|&r| rank < r) {
                rank_at.insert(c, rank);
                self.cells.insert(c, id);
            }
        }
        self.by_comp = nets.iter().enumerate().map(|(k, x)| (x.comp, k)).collect();
        self.list = nets;
    }
}

impl World {
    /// [walk-networks] Find the walk graph's networks again (after the
    /// graph was rebuilt; `build_walk_graph`).
    pub(super) fn refresh_walk_networks(&mut self) {
        let mut nets = std::mem::take(&mut self.walk_networks);
        let lots = &self.lots;
        let buildings = &self.buildings;
        let lot_info = |l: LotId| {
            let b = lots.get(l)?.building?;
            let bld = buildings.get(b)?;
            Some((b, bld.residents.len() as u32, bld.workers.len() as u32, bld.name.clone()))
        };
        let street_name = |p: Vec2| {
            let (sid, _, q) = self.roads.nearest_where(p, |s| !s.is_rail())?;
            (q.distance(p) < 400.0).then(|| self.streets.of(sid).map(|s| s.name.clone())).flatten()
        };
        nets.update(&self.walk, lot_info, street_name);
        self.walk_networks = nets;
    }

    /// [walk-networks] The network lot `l`'s own walk nodes (or the
    /// pavement in front) are on.
    pub fn walk_network_of_lot(&self, l: LotId) -> Option<&WalkNetwork> {
        let b = self.lots.get(l)?.building?;
        self.walk_networks.list.iter().find(|n| n.buildings.contains(&b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two strips of nodes, apart: `join` links them.
    fn two(join: bool, version: u64) -> WalkGraph {
        let mut g = WalkGraph::default();
        g.version = version;
        // Left: 6 nodes in a row; right: 3 nodes, 200 m east.
        for i in 0..6 {
            g.add_node(Vec2::new(i as f32 * 10.0, 0.0), None);
        }
        for i in 0..3 {
            g.add_node(Vec2::new(200.0 + i as f32 * 10.0, 0.0), None);
        }
        for i in 0..5 {
            g.link(i, i + 1, false);
        }
        g.link(6, 7, false);
        g.link(7, 8, false);
        if join {
            // A path across the gap, with nodes of its own.
            let a = g.add_node(Vec2::new(100.0, 0.0), None);
            let b = g.add_node(Vec2::new(150.0, 0.0), None);
            g.link(5, a, false);
            g.link(a, b, false);
            g.link(b, 6, false);
        }
        g.compute_components();
        g
    }

    /// [walk-networks] The larger network is id 1; joined, the whole takes
    /// the larger one's id; split again, the larger piece keeps it and the
    /// smaller gets a new one (not its old colour back from someone else).
    #[test]
    fn networks_keep_their_ids_and_a_merge_takes_the_larger_ones() {
        let mut nets = WalkNetworks::default();
        let none = |_: LotId| None;
        let street = |_: Vec2| None;
        nets.update(&two(false, 1), none, street);
        assert_eq!(nets.list.len(), 2);
        assert_eq!((nets.list[0].id, nets.list[0].edges), (1, 5), "the larger is first and id 1");
        let small = nets.list[1].id;
        assert_ne!(small, 1);
        assert_ne!(network_color(1), network_color(small), "told apart by colour");
        nets.update(&two(true, 2), none, street);
        assert_eq!(nets.list.len(), 1, "joined");
        assert_eq!(nets.list[0].id, 1, "the joined network keeps the larger one's id");
        nets.update(&two(false, 3), none, street);
        assert_eq!(nets.list[0].id, 1, "the larger piece keeps it");
        assert!(nets.list[1].id != 1, "the other is told apart");
        let g = two(false, 3);
        assert_eq!(nets.of_node(&g, 0).map(|n| n.id), Some(1));
        assert_eq!(nets.of_node(&g, 7).map(|n| n.id), Some(nets.list[1].id));
    }
}
