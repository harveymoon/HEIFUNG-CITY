//! Ramp speed profiles: where a ramp leaves or joins its road, cars ease
//! their speed over the taper rather than braking at the nose.

use super::*;

impl LaneNetwork {
    /// Ease the limits along every highway slip road: from the highway's
    /// speed where it leaves (or joins) the carriageway to the street's
    /// where it meets the town, smoothly over its whole length, capped on
    /// its bends by what the curve allows. A ramp is several pieces joined
    /// by connectors; each lane gets the profile's value at its start and
    /// end (`Lane::limit_at` interpolates between), so the limit a driver
    /// sees changes gradually instead of stepping at every joint (before,
    /// the highway-to-ramp connector was a 6 m/s junction turn and every
    /// bend of a curved ramp another limit).
    pub(super) fn ramp_profiles(&mut self, roads: &RoadNetwork) {
        let n = self.lanes.len();
        // Connectors along a ramp: between two ramp lanes, or between a
        // ramp lane and the highway.
        for i in 0..n {
            if !self.lanes[i].is_connector() {
                continue;
            }
            let (Some(&a), Some(&b)) = (self.lanes[i].prev.first(), self.lanes[i].next.first()) else { continue };
            let (la, lb) = (&self.lanes[a as usize], &self.lanes[b as usize]);
            if (la.ramp || lb.ramp) && (la.ramp || la.highway) && (lb.ramp || lb.highway) {
                self.lanes[i].ramp = true;
            }
        }
        if !self.lanes.iter().any(|l| l.ramp) {
            return;
        }
        // Distance from the ramp's upstream boundary to each lane's start
        // (Dijkstra forward), and from each lane's end to its downstream
        // boundary (backward); and whether those boundaries are the highway.
        let inf = f32::INFINITY;
        let mut up = vec![(inf, false); n];
        let mut down = vec![(inf, false); n];
        let key = |d: f32| (d * 1000.0) as u64;
        let mut heap: BinaryHeap<std::cmp::Reverse<(u64, LaneId)>> = BinaryHeap::new();
        for i in 0..n {
            if !self.lanes[i].ramp {
                continue;
            }
            if let Some(&p) = self.lanes[i].prev.iter().find(|&&p| !self.lanes[p as usize].ramp) {
                up[i] = (0.0, self.lanes[p as usize].highway);
                heap.push(std::cmp::Reverse((0, i as LaneId)));
            }
        }
        while let Some(std::cmp::Reverse((_, l))) = heap.pop() {
            let (d, hw) = up[l as usize];
            let len = self.lanes[l as usize].length;
            for k in 0..self.lanes[l as usize].next.len() {
                let mi = self.lanes[l as usize].next[k] as usize;
                if self.lanes[mi].ramp && d + len < up[mi].0 {
                    up[mi] = (d + len, hw);
                    heap.push(std::cmp::Reverse((key(d + len), mi as LaneId)));
                }
            }
        }
        for i in 0..n {
            if !self.lanes[i].ramp {
                continue;
            }
            if let Some(&x) = self.lanes[i].next.iter().find(|&&x| !self.lanes[x as usize].ramp) {
                down[i] = (0.0, self.lanes[x as usize].highway);
                heap.push(std::cmp::Reverse((0, i as LaneId)));
            }
        }
        while let Some(std::cmp::Reverse((_, l))) = heap.pop() {
            let (d, hw) = down[l as usize];
            // From the end of a lane feeding `l`, on through all of `l`.
            let len = self.lanes[l as usize].length;
            for k in 0..self.lanes[l as usize].prev.len() {
                let mi = self.lanes[l as usize].prev[k] as usize;
                if self.lanes[mi].ramp && d + len < down[mi].0 {
                    down[mi] = (d + len, hw);
                    heap.push(std::cmp::Reverse((key(d + len), mi as LaneId)));
                }
            }
        }
        // How close to the highway a point is, 1 at the carriageway and 0
        // where the ramp meets the street, eased (smoothstep) so the change
        // in limit starts and ends gently.
        let profile = |to_up: f32, to_down: f32, up_hw: bool, down_hw: bool| -> f32 {
            let total = (to_up + to_down).max(1e-3);
            let k = match (up_hw, down_hw) {
                (true, true) => 1.0,
                (false, false) => 0.0,
                (true, false) => to_down / total,
                (false, true) => to_up / total,
            };
            let k = k.clamp(0.0, 1.0);
            let k = k * k * (3.0 - 2.0 * k);
            ROAD_SPEED + (HIGHWAY_SPEED - ROAD_SPEED) * k
        };
        // Bends: at a ramp node joining two pieces, the radius of the curve
        // they stand for caps the limit around it.
        let bend_cap = |node: NodeId| -> f32 {
            let nd = roads.node(node);
            let segs: Vec<SegmentId> = roads.streets_at(node).collect();
            if segs.len() != 2 {
                return f32::INFINITY;
            }
            let away = |s: SegmentId| {
                let (a, b) = roads.segment_points(s);
                if a.distance(nd.pos) < b.distance(nd.pos) { b - a } else { a - b }
            };
            let (u, v) = (away(segs[0]), away(segs[1]));
            let turn = std::f32::consts::PI - u.angle_to(v).abs();
            if turn < 0.02 {
                return f32::INFINITY;
            }
            let r = 0.5 * (u.length().min(60.0) + v.length().min(60.0)) / turn;
            (RAMP_LATERAL * r).sqrt().max(CONNECTOR_SPEED)
        };
        // The bend caps, each at its place along its ramp (distance from
        // the ramp's upstream end), by ramp: the ramp lanes linked to each
        // other form one component.
        let mut comp: Vec<usize> = (0..n).collect();
        fn root(c: &mut [usize], mut i: usize) -> usize {
            while c[i] != i {
                c[i] = c[c[i]];
                i = c[i];
            }
            i
        }
        for i in 0..n {
            if !self.lanes[i].ramp {
                continue;
            }
            for k in 0..self.lanes[i].next.len() {
                let j = self.lanes[i].next[k] as usize;
                if self.lanes[j].ramp {
                    let (a, b) = (root(&mut comp, i), root(&mut comp, j));
                    comp[a.max(b)] = a.min(b);
                }
            }
        }
        let mut caps: HashMap<usize, Vec<(f32, f32)>> = HashMap::new();
        for i in 0..n {
            if let (true, LaneKind::Connector { node, .. }) = (self.lanes[i].ramp && up[i].0.is_finite(), &self.lanes[i].kind) {
                let cap = bend_cap(*node);
                if cap.is_finite() {
                    let r = root(&mut comp, i);
                    caps.entry(r).or_default().push((up[i].0 + self.lanes[i].length * 0.5, cap));
                }
            }
        }
        // [interchange] Bends close together are one curve (a ramp rounded
        // in a few pieces): the lowest cap holds from its first bend to its
        // last, so the limit does not rise between them and fall again.
        let caps: HashMap<usize, Vec<(f32, f32, f32)>> = caps
            .into_iter()
            .map(|(r, mut cs)| {
                cs.sort_by(|a, b| a.0.total_cmp(&b.0));
                let mut out: Vec<(f32, f32, f32)> = Vec::new();
                for (x, c) in cs {
                    match out.last_mut() {
                        Some(last) if x - last.1 < 30.0 => {
                            last.1 = x;
                            last.2 = last.2.min(c);
                        }
                        _ => out.push((x, x, c)),
                    }
                }
                (r, out)
            })
            .collect();
        for i in 0..n {
            if !self.lanes[i].ramp || !up[i].0.is_finite() || !down[i].0.is_finite() {
                continue;
            }
            let len = self.lanes[i].length;
            let (uh, dh) = (up[i].1, down[i].1);
            let r = root(&mut comp, i);
            // The profile, or less near a bend: a cap holds at its bend and
            // eases off either side at the rate drivers brake for limits,
            // so the limit never steps.
            let at = |x: f32, v: f32| {
                caps.get(&r).map_or(v, |cs| cs.iter().fold(v, |v, &(x0, x1, c)| v.min((c * c + 2.0 * ANTICIPATE_BRAKE * (x0 - x).max(x - x1).max(0.0)).sqrt())))
            };
            self.lanes[i].speed_limit = at(up[i].0, profile(up[i].0, down[i].0 + len, uh, dh));
            self.lanes[i].limit_end = at(up[i].0 + len, profile(up[i].0 + len, down[i].0, uh, dh));
            // [interchange] Off a curve's last bend the cap eases off into a
            // profile still falling: an off-ramp's limit never rises along a
            // lane (nor an on-ramp's falls).
            let (a, b) = (self.lanes[i].speed_limit, self.lanes[i].limit_end);
            self.lanes[i].limit_end = if uh && !dh { b.min(a) } else if dh && !uh { b.max(a) } else { b };
        }
    }
}
