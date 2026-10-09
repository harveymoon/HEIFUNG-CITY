//! Grounds: parks, plazas and campuses walked through, their paths joined
//! to the pavements, and their benches.

use super::*;

/// [park-paths] What kind of public ground a walkway network belongs to.
/// Saved with a path's attachment, so kinds keep their order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum GroundsKind {
    #[default]
    Park,
    /// A monument's paved plaza.
    Plaza,
    Stadium,
    /// A school or college campus.
    Campus,
    /// A railway station's forecourt.
    Station,
    /// A public beach (`feat/waterfront` may register them).
    Beach,
}

impl GroundsKind {
    pub fn key(self) -> &'static str {
        match self {
            GroundsKind::Park => "park",
            GroundsKind::Plaza => "plaza",
            GroundsKind::Stadium => "stadium",
            GroundsKind::Campus => "campus",
            GroundsKind::Station => "station",
            GroundsKind::Beach => "beach",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GroundsKind::Park => "park",
            GroundsKind::Plaza => "plaza",
            GroundsKind::Stadium => "stadium grounds",
            GroundsKind::Campus => "campus",
            GroundsKind::Station => "station forecourt",
            GroundsKind::Beach => "beach",
        }
    }
}

/// [park-paths] A piece of public ground with walkways a player path may
/// join: its outline, the gates on its edge a path may be drawn to, and
/// the lot whose walk nodes (its loop, a park's paths) it is joined to.
/// Generic so any ground with a walkway network (a beach) can register.
#[derive(Clone, Debug)]
pub struct Grounds {
    pub kind: GroundsKind,
    pub lot: Option<LotId>,
    pub outline: Vec<Vec2>,
    /// Snap points on (just outside) the edge.
    pub gates: Vec<Vec2>,
}

/// [park-paths] One snap point: where it is, whose ground, and whether a
/// player path is joined there (`node`: the grounds' walk node it joins).
#[derive(Clone, Debug)]
pub struct GroundsJunction {
    pub pos: Vec2,
    pub kind: GroundsKind,
    pub lot: Option<LotId>,
    pub attached: bool,
    pub node: Option<u32>,
}

/// [park-paths] A path end this close to a grounds' outline (or inside it)
/// is joined to it; further out, the grounds have gone.
pub const GROUNDS_REACH: f32 = 4.0;

/// [park-paths] Gates are put this far outside the edge, clear of it.
pub const GATE_OUT: f32 = 1.6;

impl WalkGraph {
    /// The benches in a lot (node, lot, facing), in the order they were laid.
    pub fn benches_in(&self, lot: LotId) -> impl Iterator<Item = &(u32, LotId, f32)> + '_ {
        let idx: &[u32] = self.bench_lots.get(&lot).map_or(&[], |v| v.as_slice());
        let indexed = !self.bench_lots.is_empty() || self.benches.is_empty();
        // A graph put together by hand (tests) has no index: scan it.
        let scan = (!indexed).then(|| self.benches.iter().filter(move |b| b.1 == lot)).into_iter().flatten();
        idx.iter().map(|&k| &self.benches[k as usize]).chain(scan)
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

    /// [park-paths] Join player paths to the public grounds they were drawn
    /// to, and list every grounds' snap points. `links` are the path ends
    /// attached to grounds; each is joined (a walk edge from the path's end
    /// to the nearest walk node of the grounds) when grounds still lie
    /// within `GROUNDS_REACH` of it: the returned flags say which were.
    /// Gates within a road end's snap reach (`END_SNAP_RADIUS`) of a kerb
    /// are left out: a path drawn there joins the road instead.
    pub fn add_grounds(&mut self, roads: &RoadNetwork, grounds: &[Grounds], links: &[Vec2]) -> Vec<bool> {
        let mut joined = Vec::with_capacity(links.len());
        for &p in links {
            let near = grounds
                .iter()
                .enumerate()
                .map(|(i, g)| (i, if geom::point_in_polygon(p, &g.outline) { 0.0 } else { geom::distance_to_outline(p, &g.outline) }))
                .filter(|(_, d)| *d <= GROUNDS_REACH)
                .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let Some((gi, _)) = near else {
                joined.push(false);
                continue;
            };
            let g = &grounds[gi];
            // The path's own end: a street-side node (no lot) at the end.
            let end = self.nearest_matching(p, 1.0, |n| self.nodes[n as usize].lot.is_none());
            let inside = |n: u32| match g.lot {
                Some(l) => self.nodes[n as usize].lot == Some(l),
                None => geom::point_in_polygon(self.nodes[n as usize].pos, &g.outline),
            };
            // A park's own walkways first (the entrance laid out to meet
            // the path), else any walk node of the grounds.
            let on_paths = |n: u32| g.kind != GroundsKind::Park || self.adj[n as usize].iter().any(|e| e.park);
            let target = self.nearest_matching(p, 40.0, |n| inside(n) && on_paths(n) && Some(n) != end).or_else(|| self.nearest_matching(p, 40.0, |n| inside(n) && Some(n) != end));
            if let (Some(a), Some(b)) = (end, target) {
                self.link_kind(a, b, true, g.kind == GroundsKind::Park);
                // [under-construction] Marked, so it shuts with the grounds.
                for (x, y) in [(a, b), (b, a)] {
                    if let Some(e) = self.adj[x as usize].iter_mut().rev().find(|e| e.to == y) {
                        e.gate = true;
                    }
                }
            }
            self.junctions.push(GroundsJunction { pos: p, kind: g.kind, lot: g.lot, attached: true, node: target });
            joined.push(true);
        }
        // Clear of the kerb by more than a road end's snap reach: by the
        // pavement people walk in from the street anyway.
        let reach = crate::roads::END_SNAP_RADIUS + 3.0;
        for g in grounds {
            for &q in &g.gates {
                if links.iter().any(|l| l.distance(q) < 3.0) {
                    continue;
                }
                if roads.nearest_where(q, |s| !s.is_rail()).is_some_and(|(sid, _, r)| r.distance(q) - roads.segment(sid).half_width() < reach) {
                    continue;
                }
                self.junctions.push(GroundsJunction { pos: q, kind: g.kind, lot: g.lot, attached: false, node: None });
            }
        }
        joined
    }

    /// [park-paths] The snap point nearest `p` within `r`.
    pub fn junction_near(&self, p: Vec2, r: f32) -> Option<&GroundsJunction> {
        self.junctions.iter().filter(|j| j.pos.distance(p) <= r).min_by(|a, b| a.pos.distance(p).total_cmp(&b.pos.distance(p)))
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
