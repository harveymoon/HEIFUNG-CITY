//! Service coverage: how far each station reaches along the roads, lot by lot.

use super::*;

impl World {
    /// Where a lot meets the street network: the car point, or the
    /// entrance on its frontage for lots on a path.
    pub(in crate::world) fn lot_access(&self, lot: &Lot) -> Option<(SegmentId, Vec2)> {
        let (seg, p) = match lot.car_segment {
            Some(s) => (s, lot.car_point),
            None => (lot.front_segment, lot.entrance()),
        };
        self.roads.try_segment(seg).map(|_| (seg, p))
    }

    pub(super) fn coverage_signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.roads.version.hash(&mut h);
        self.lots.len().hash(&mut h);
        for (id, b) in self.buildings.iter() {
            if let Some(s) = b.service {
                if s.kind.coverage_index().is_some() {
                    (id, s.tier.index(), b.is_derelict(), b.operating(), b.lot).hash(&mut h);
                }
            }
        }
        h.finish()
    }

    /// Coverage of every lot by every covering service: the best road
    /// budget left (a station's radius minus the road distance) over all
    /// stations of the kind, faded over the last `COVERAGE_FADE` metres.
    pub fn refresh_coverage(&mut self) {
        let mut out: HashMap<LotId, [f32; 4]> = HashMap::new();
        let mut sources: [Vec<(SegmentId, Vec2, f32)>; 4] = Default::default();
        for b in self.buildings.values() {
            let Some(s) = b.service else { continue };
            let Some(k) = s.kind.coverage_index() else { continue };
            if b.is_derelict() || !b.operating() {
                continue;
            }
            if let Some((seg, p)) = self.lots.get(b.lot).and_then(|l| self.lot_access(l)) {
                sources[k].push((seg, p, s.kind.radius(s.tier)));
            }
        }
        for (k, src) in sources.iter().enumerate() {
            for (lid, cov) in self.coverage_from(src) {
                out.entry(lid).or_insert([0.0; 4])[k] = cov;
            }
        }
        self.services.coverage = out;
        self.services.coverage_key = Some(self.coverage_signature());
        // Share of homes covered, per kind.
        let homes: Vec<LotId> = self.buildings.values().filter(|b| b.homes > 0 && b.operating()).map(|b| b.lot).collect();
        for k in 0..4 {
            let covered = homes.iter().filter(|l| self.services.coverage.get(l).is_some_and(|c| c[k] >= COVERED)).count();
            self.services.stats.coverage[k] = if homes.is_empty() { 0.0 } else { covered as f32 / homes.len() as f32 };
        }
    }

    /// Lots reached from `src` (road access point, budget in metres) along
    /// the street network, with their coverage `0..1` (the best budget left
    /// on arrival, faded over the last `COVERAGE_FADE` metres). Rail does
    /// not carry coverage. Lots with none are left out.
    pub(super) fn coverage_from(&self, src: &[(SegmentId, Vec2, f32)]) -> Vec<(LotId, f32)> {
        if src.is_empty() {
            return Vec::new();
        }
        let n = self.roads.nodes.len();
        let mut best = vec![f32::NEG_INFINITY; n];
        let mut heap = BinaryHeap::new();
        for &(seg, p, r) in src {
            let s = self.roads.segment(seg);
            for node in [s.a, s.b] {
                let budget = r - p.distance(self.roads.node(node).pos);
                if budget > best[node.0 as usize] {
                    best[node.0 as usize] = budget;
                    heap.push(Budget(budget, node.0));
                }
            }
        }
        while let Some(Budget(b, node)) = heap.pop() {
            if b < best[node as usize] || b <= 0.0 {
                continue;
            }
            let here = self.roads.node(NodeId(node)).pos;
            for (nb, sid) in self.roads.neighbors(NodeId(node)) {
                if self.roads.segment(sid).is_rail() {
                    continue;
                }
                let nbudget = b - here.distance(self.roads.node(nb).pos);
                if nbudget > best[nb.0 as usize] {
                    best[nb.0 as usize] = nbudget;
                    heap.push(Budget(nbudget, nb.0));
                }
            }
        }
        let mut out = Vec::new();
        for (lid, lot) in self.lots.iter() {
            let Some((seg, p)) = self.lot_access(lot) else { continue };
            let s = self.roads.segment(seg);
            let mut rem = [s.a, s.b].iter().map(|nd| best[nd.0 as usize] - p.distance(self.roads.node(*nd).pos)).fold(f32::NEG_INFINITY, f32::max);
            for &(sseg, sp, r) in src {
                if sseg == seg {
                    rem = rem.max(r - p.distance(sp));
                }
            }
            let cov = (rem / COVERAGE_FADE).clamp(0.0, 1.0);
            if cov > 0.0 {
                out.push((lid, cov));
            }
        }
        out
    }

    /// [build-highlight] The lots one covering station reaches by road and
    /// how well (`0..1`), as `refresh_coverage` counts them for it alone:
    /// its own service area. Empty for a station that covers nothing (not a
    /// covering kind, derelict, being built, or off the street network).
    pub fn service_reach(&self, station: BuildingId) -> Vec<(LotId, f32)> {
        let Some(b) = self.buildings.get(station) else { return Vec::new() };
        let Some(s) = b.service else { return Vec::new() };
        if s.kind.coverage_index().is_none() || b.is_derelict() || !b.operating() {
            return Vec::new();
        }
        let Some((seg, p)) = self.lots.get(b.lot).and_then(|l| self.lot_access(l)) else { return Vec::new() };
        self.coverage_from(&[(seg, p, s.kind.radius(s.tier))])
    }

    /// Coverage of the lot nearest `p` by a covering service kind.
    pub fn coverage_at(&self, p: Vec2, kind: ServiceKind) -> f32 {
        self.lots.iter().filter(|(_, l)| l.usable()).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map_or(0.0, |(id, _)| self.services.coverage_of(id, kind))
    }
}
