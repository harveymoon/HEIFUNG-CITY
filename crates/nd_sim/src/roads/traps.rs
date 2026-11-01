//! [r5-fixes] Streets with no way out: one-way streets leading into a
//! street (or a knot of streets) that no road leads out of again. The
//! traffic's own rule (`traffic::LaneNetwork::mark_traps`) stops routes
//! going in; this finds the same pockets on the streets alone, cheaply
//! enough for the Road tool to warn while a road is drawn.

use super::*;

/// A pocket of streets traffic can drive into but never out of.
#[derive(Clone, Debug, PartialEq)]
pub struct Trap {
    /// Its street pieces (and the one-way pieces that lead only into it).
    pub segments: Vec<SegmentId>,
    /// About its middle, for the message and the map.
    pub at: Vec2,
}

impl RoadNetwork {
    /// The pockets of streets with no way out. Judged on the junctions:
    /// a junction is in a pocket when no street leads from it, however
    /// round about, back to the main network of its part of the map (the
    /// biggest set of junctions all reachable from one another) or to a
    /// one-way end (a road off the map). A two-way street can be driven
    /// back along, so a plain dead end is no pocket. A street piece is in
    /// one when every way along it ends there.
    pub fn traps(&self) -> Vec<Trap> {
        let n = self.nodes.len();
        let mut out_edges: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut in_edges: Vec<Vec<usize>> = vec![Vec::new(); n];
        let drivable = |s: &Segment| s.alive && s.is_street();
        for s in self.segments.iter().filter(|s| drivable(s)) {
            let (a, b) = (s.a.0 as usize, s.b.0 as usize);
            if s.lanes_fwd > 0 {
                out_edges[a].push(b);
                in_edges[b].push(a);
            }
            if s.lanes_bwd > 0 {
                out_edges[b].push(a);
                in_edges[a].push(b);
            }
        }
        let used: Vec<bool> = (0..n).map(|i| !out_edges[i].is_empty() || !in_edges[i].is_empty()).collect();
        // Strongly connected components (Tarjan, without recursion).
        let mut index = vec![u32::MAX; n];
        let mut low = vec![0u32; n];
        let mut on = vec![false; n];
        let mut comp = vec![u32::MAX; n];
        let mut stack: Vec<usize> = Vec::new();
        let (mut next, mut ncomp) = (0u32, 0u32);
        for root in (0..n).filter(|&i| used[i]) {
            if index[root] != u32::MAX {
                continue;
            }
            let mut call: Vec<(usize, usize)> = vec![(root, 0)];
            index[root] = next;
            low[root] = next;
            next += 1;
            stack.push(root);
            on[root] = true;
            while let Some(&(v, k)) = call.last() {
                if k < out_edges[v].len() {
                    let w = out_edges[v][k];
                    if let Some(top) = call.last_mut() {
                        top.1 += 1;
                    }
                    if index[w] == u32::MAX {
                        index[w] = next;
                        low[w] = next;
                        next += 1;
                        stack.push(w);
                        on[w] = true;
                        call.push((w, 0));
                    } else if on[w] {
                        low[v] = low[v].min(index[w]);
                    }
                } else {
                    call.pop();
                    if let Some(&(u, _)) = call.last() {
                        low[u] = low[u].min(low[v]);
                    }
                    if low[v] == index[v] {
                        while let Some(w) = stack.pop() {
                            on[w] = false;
                            comp[w] = ncomp;
                            if w == v {
                                break;
                            }
                        }
                        ncomp += 1;
                    }
                }
            }
        }
        // Weakly joined parts, and the biggest component in each.
        let mut part: Vec<usize> = (0..n).collect();
        fn find(p: &mut [usize], mut x: usize) -> usize {
            while p[x] != x {
                p[x] = p[p[x]];
                x = p[x];
            }
            x
        }
        for v in 0..n {
            for &w in &out_edges[v] {
                let (a, b) = (find(&mut part, v), find(&mut part, w));
                if a != b {
                    part[a.max(b)] = a.min(b);
                }
            }
        }
        let mut size = vec![0u32; ncomp as usize];
        for v in (0..n).filter(|&v| used[v]) {
            size[comp[v] as usize] += 1;
        }
        let mut main: HashMap<usize, u32> = HashMap::new();
        for v in (0..n).filter(|&v| used[v]) {
            let p = find(&mut part, v);
            let c = comp[v];
            let e = main.entry(p).or_insert(c);
            if size[c as usize] > size[*e as usize] || (size[c as usize] == size[*e as usize] && c < *e) {
                *e = c;
            }
        }
        // Back from the main networks and the one-way ends.
        let mut reach = vec![false; n];
        let mut todo: Vec<usize> = (0..n).filter(|&v| used[v] && (out_edges[v].is_empty() || main.get(&find(&mut part, v)) == Some(&comp[v]))).collect();
        for &v in &todo {
            reach[v] = true;
        }
        while let Some(v) = todo.pop() {
            for &u in &in_edges[v] {
                if !reach[u] {
                    reach[u] = true;
                    todo.push(u);
                }
            }
        }
        // Street pieces every way along which ends in a pocket, grouped
        // into pockets by the junctions they share.
        let trapped: Vec<usize> = self
            .segments
            .iter()
            .enumerate()
            .filter(|(_, s)| drivable(s))
            .filter(|(_, s)| {
                let (a, b) = (s.a.0 as usize, s.b.0 as usize);
                (s.lanes_fwd == 0 || !reach[b]) && (s.lanes_bwd == 0 || !reach[a])
            })
            .map(|(i, _)| i)
            .collect();
        if trapped.is_empty() {
            return Vec::new();
        }
        let mut group: Vec<usize> = (0..n).collect();
        for &i in &trapped {
            let s = &self.segments[i];
            let (a, b) = (find(&mut group, s.a.0 as usize), find(&mut group, s.b.0 as usize));
            if a != b {
                group[a.max(b)] = a.min(b);
            }
        }
        let mut pockets: Vec<(usize, Trap)> = Vec::new();
        for &i in &trapped {
            let s = &self.segments[i];
            let g = find(&mut group, s.a.0 as usize);
            let k = match pockets.iter().position(|(x, _)| *x == g) {
                Some(k) => k,
                None => {
                    pockets.push((g, Trap { segments: Vec::new(), at: Vec2::ZERO }));
                    pockets.len() - 1
                }
            };
            pockets[k].1.segments.push(SegmentId(i as u32));
        }
        pockets
            .into_iter()
            .map(|(_, mut t)| {
                // The middle, weighted by length: the same however its
                // streets are cut into pieces (a junction added on one).
                let (sum, len) = t.segments.iter().map(|&s| self.segment_points(s)).fold((Vec2::ZERO, 0.0f32), |(sum, len), (a, b)| {
                    let l = a.distance(b).max(0.01);
                    (sum + (a + b) * 0.5 * l, len + l)
                });
                t.at = sum / len;
                t
            })
            .collect()
    }
}

/// [r5-fixes] The Road tool's words for a pocket: how many street pieces,
/// where, and what to do about it.
pub fn trap_warning(t: &Trap) -> String {
    let n = t.segments.len();
    format!(
        "no way out at ({:.0}, {:.0}): one-way streets lead into {n} street piece{} that no road leads out of; traffic will not be routed in (make one of them two-way or turn one round)",
        t.at.x,
        t.at.y,
        if n == 1 { "" } else { "s" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The traffic test's pocket (`a_pocket_fed_only_by_one_way_streets_is_never_routed_into`):
    /// a two-way street whose ends meet only one-way streets pointing into
    /// it. The streets alone find it, with the lanes' own rule agreeing.
    #[test]
    fn a_pocket_fed_only_by_one_way_streets_is_found_on_the_streets() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        assert!(roads.traps().is_empty(), "a plain street is no pocket");
        roads.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(-100.0, 100.0)], 1, 0);
        let pocket = roads.add_polyline(&[Vec2::new(-100.0, 100.0), Vec2::new(100.0, 100.0)], 1, 1);
        assert_eq!(roads.traps().len(), 1, "one one-way street in already makes the pocket: up and down it, never out");
        roads.add_polyline(&[Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)], 1, 0);
        let traps = roads.traps();
        assert_eq!(traps.len(), 1, "{traps:?}");
        assert!(pocket.iter().all(|s| traps[0].segments.contains(s)), "the pocket's street is in it");
        assert!(traps[0].at.distance(Vec2::new(0.0, 80.0)) < 40.0, "about its middle: {:?}", traps[0].at);
        let net = {
            let mut n = crate::traffic::LaneNetwork::default();
            n.rebuild(&roads);
            n
        };
        for s in &pocket {
            assert!(net.segment_lanes[s].iter().all(|&l| net.trap[l as usize]), "the lanes call it a trap too");
        }
        // One of the one-way streets turned two-way: a way out.
        let mut out = RoadNetwork::default();
        out.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        out.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(-100.0, 100.0)], 1, 0);
        out.add_polyline(&[Vec2::new(-100.0, 100.0), Vec2::new(100.0, 100.0)], 1, 1);
        out.add_polyline(&[Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)], 1, 1);
        assert!(out.traps().is_empty());
    }

    /// A ring of one-way streets all running the same way round is a way
    /// about, not a trap; a two-way dead end off it neither.
    #[test]
    fn a_one_way_loop_and_a_dead_end_are_no_pocket() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0), Vec2::new(200.0, 200.0), Vec2::new(0.0, 200.0), Vec2::new(0.0, 0.0)], 1, 0);
        roads.add_polyline(&[Vec2::new(200.0, 100.0), Vec2::new(400.0, 100.0)], 1, 1);
        assert!(roads.traps().is_empty(), "{:?}", roads.traps());
    }
}
