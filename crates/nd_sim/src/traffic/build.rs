//! The lane network built from the road network: lanes per segment,
//! junctions with their approaches and connector lanes.

use super::*;

#[derive(Clone, Debug)]
pub struct Intersection {
    pub node: NodeId,
    pub radius: f32,
    /// Streets meeting here, exits included.
    pub approaches: usize,
    /// Signal phases, each the approaches (with incoming lanes) that share
    /// a green: opposite approaches go together, left turns yielding to
    /// oncoming traffic. A one-way exit never gets a phase of its own in
    /// which nothing can move.
    pub phases: Vec<Vec<usize>>,
    /// Index into `phases` of the group that has the green.
    pub phase: usize,
    /// The approach straight across, per approach, if it feeds the junction.
    pub opposite: Vec<Option<usize>>,
    /// Seconds of protected left arrow at the start of the current green
    /// (0 for none): only left turns go, oncoming traffic has red.
    pub arrow: f32,
    /// Left turns made here and the time their drivers stood waiting.
    pub left_turns: u32,
    pub left_wait_sum: f32,
    pub left_wait_max: f32,
    pub timer: f32,
    pub connectors: Vec<(LaneId, usize)>,
    /// Road lanes ending here, with their approach index.
    pub incoming: Vec<(LaneId, usize)>,
    pub signalized: bool,
    /// [tools] Green seconds per phase (the player's, from the
    /// Intersection editor; `GREEN_SECONDS` by default).
    pub greens: Vec<f32>,
    /// [tools] When the protected left arrow runs, and for how long.
    pub arrow_mode: crate::junctions::LeftArrow,
    pub arrow_len: f32,
    /// [tools] Set by a junction rule: the player chose signals or none.
    pub ruled: bool,
}

impl Intersection {
    /// Green seconds of the phase now running.
    pub fn green(&self) -> f32 {
        self.green_of(self.phase)
    }

    /// Green seconds of phase `k`.
    pub fn green_of(&self, k: usize) -> f32 {
        self.greens.get(k).copied().filter(|g| *g > 0.0).unwrap_or(GREEN_SECONDS)
    }

    /// The default green, for the editor.
    pub const DEFAULT_GREEN: f32 = GREEN_SECONDS;
    /// The default protected arrow, for the editor.
    pub const DEFAULT_ARROW: f32 = ARROW_SECONDS;
}

/// The lanes of one street arriving at a junction, with their arrows.
#[derive(Clone, Debug)]
pub struct Approach {
    pub node: NodeId,
    pub segment: SegmentId,
    /// Leftmost first.
    pub lanes: Vec<LaneId>,
    /// Movements this junction offers from here.
    pub exits: u8,
    pub arrows: Vec<u8>,
    /// Set by the player (`Traffic::lane_use`): not revised from demand.
    pub fixed: bool,
    /// Recent left, straight and right turns, decaying.
    pub counts: [f32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct LaneNetwork {
    pub lanes: Vec<Lane>,
    pub intersections: Vec<Intersection>,
    pub segment_lanes: HashMap<SegmentId, Vec<LaneId>>,
    pub node_radius: Vec<f32>,
    /// [interchange] Lanes of an arm ending further out than its node's
    /// radius (`walk::stop_back`: a sharp fork's stop line), by (node, arm).
    pub stop_back: HashMap<(NodeId, SegmentId), f32>,
    /// Index into `intersections` per node, `usize::MAX` for none.
    pub node_intersection: Vec<usize>,
    pub approaches: Vec<Approach>,
    pub version: u64,
    /// Bumped whenever lane arrows change (for drawing them).
    pub arrows_version: u64,
    /// [fix/roads-tools] Bumped whenever a junction's signals may have been
    /// switched on or off without a lane rebuild (the Intersection editor),
    /// so the signal heads and poles are drawn again.
    pub signals_version: u64,
    /// Roundabout lanes and junctions (see `apply_roundabouts`).
    pub rings: RingLanes,
    /// [tolls] Some lane has a toll booth (skips the toll checks when none does).
    pub tolls: bool,
    /// [toll-inspect] Every booth and where it stands (set with the lanes'
    /// tolls), for telling who drove round one.
    pub toll_points: Vec<(u32, Vec2)>,
    /// [rail-crossing] Some connector is closed for a level crossing (skips
    /// the crossing checks when none is).
    pub rail_closed_any: bool,
}

/// [r2-traffic] A street wide enough for a U-turn at its junctions: two
/// or more lanes each way, not a highway, slip road or roundabout.
pub(super) fn uturn_street(roads: &RoadNetwork, seg: SegmentId) -> bool {
    let s = roads.segment(seg);
    s.lanes_fwd >= 2 && s.lanes_bwd >= 2 && !s.is_highway() && !s.ramp && !s.ring
}

impl LaneNetwork {
    pub fn rebuild(&mut self, roads: &RoadNetwork) {
        self.lanes.clear();
        self.intersections.clear();
        self.segment_lanes.clear();
        self.node_intersection = vec![usize::MAX; roads.nodes.len()];
        self.approaches.clear();
        self.stop_back.clear();
        self.version += 1;
        self.arrows_version += 1;

        // Intersection radius per node: enough to fit the widest road.
        self.node_radius = roads
            .nodes
            .iter()
            .map(|n| {
                let alive: Vec<_> = n.segments.iter().filter(|&&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
                let w = alive.iter().map(|&&s| roads.segment(s).half_width()).fold(0.0, f32::max);
                match alive.len() {
                    0 => 0.0,
                    1 => w * 0.8,
                    2 => {
                        // A bend: the sharper it is, the more room the turn needs.
                        let (sa, sb) = (*alive[0], *alive[1]);
                        let (a0, a1) = roads.segment_points(sa);
                        let (b0, b1) = roads.segment_points(sb);
                        let here = n.pos;
                        let da = (if a0.distance(here) < a1.distance(here) { a1 - a0 } else { a0 - a1 }).normalize_or_zero();
                        let db = (if b0.distance(here) < b1.distance(here) { b1 - b0 } else { b0 - b1 }).normalize_or_zero();
                        // da and db both point away from the node; straight through means da . db = -1.
                        let bend = (1.0 + da.dot(db)).clamp(0.0, 2.0) * 0.5; // 0 straight, 1 hairpin
                        (0.6 + (w + 1.0) * bend).min(w + 1.0)
                    }
                    _ => w + 1.0,
                }
            })
            .collect();
        // [rail-crossing] At a level crossing the lanes end at the stop
        // line, clear of the track: the connector spans the crossing.
        for (ni, r) in self.node_radius.iter_mut().enumerate() {
            if let Some(stop) = crate::level_crossing::stop_radius(roads, NodeId(ni as u32)) {
                *r = r.max(stop);
            }
        }
        self.rail_closed_any = false;

        for (sid, seg) in roads.alive_segments() {
            if !seg.is_street() {
                continue;
            }
            let (pa, pb) = roads.segment_points(sid);
            let len = pa.distance(pb);
            if len < 0.5 {
                continue;
            }
            let dir = (pb - pa) / len;
            // [interchange] At a sharp fork the lanes end at the painted stop
            // line's distance, as at any junction.
            let back_a = crate::walk::stop_back(roads, seg.a, sid, self.node_radius[seg.a.0 as usize]);
            let back_b = crate::walk::stop_back(roads, seg.b, sid, self.node_radius[seg.b.0 as usize]);
            if back_a > 0.0 {
                self.stop_back.insert((seg.a, sid), back_a);
            }
            if back_b > 0.0 {
                self.stop_back.insert((seg.b, sid), back_b);
            }
            let mut ra = self.node_radius[seg.a.0 as usize] + back_a;
            let mut rb = self.node_radius[seg.b.0 as usize] + back_b;
            if len - ra - rb < 2.0 {
                let k = ((len - 2.0) / (ra + rb).max(1e-3)).clamp(0.0, 1.0);
                ra *= k;
                rb *= k;
            }
            let p0 = pa + dir * ra;
            let p1 = pb - dir * rb;
            // Right-hand traffic. The sim plane maps to world XZ with y up, so the
            // right of a car heading `dir` is (-dir.y, dir.x). The whole lane set is
            // centred on the segment line: backward lanes on the left half, forward
            // lanes on the right half, boundary k at (k - total/2) lane widths.
            let right = Vec2::new(-dir.y, dir.x);
            let total = seg.lanes() as f32;
            let left_edge = -total * 0.5 * LANE_WIDTH;
            let mut ids = Vec::new();
            let first = self.lanes.len() as LaneId;
            let speed = if seg.is_highway() { HIGHWAY_SPEED } else { ROAD_SPEED };
            let (highway, ramp) = (seg.is_highway(), seg.ramp);
            for i in 0..seg.lanes_fwd {
                let off = right * (left_edge + (seg.lanes_bwd as f32 + i as f32 + 0.5) * LANE_WIDTH);
                let id = self.lanes.len() as LaneId;
                self.lanes.push(Lane::new(vec![p0 + off, p1 + off], LaneKind::Road { segment: sid, forward: true, index: i }, seg.b, speed));
                (self.lanes[id as usize].highway, self.lanes[id as usize].ramp) = (highway, ramp);
                ids.push(id);
            }
            for j in 0..seg.lanes_bwd {
                let off = right * (left_edge + (seg.lanes_bwd as f32 - j as f32 - 0.5) * LANE_WIDTH);
                let id = self.lanes.len() as LaneId;
                self.lanes.push(Lane::new(vec![p1 + off, p0 + off], LaneKind::Road { segment: sid, forward: false, index: j }, seg.a, speed));
                (self.lanes[id as usize].highway, self.lanes[id as usize].ramp) = (highway, ramp);
                ids.push(id);
            }
            // Index 0 is the leftmost lane in both directions.
            let (nf, nb) = (seg.lanes_fwd as LaneId, seg.lanes_bwd as LaneId);
            for (start, n) in [(first, nf), (first + nf, nb)] {
                for k in 0..n {
                    let lane = &mut self.lanes[(start + k) as usize];
                    lane.left = (k > 0).then(|| start + k - 1);
                    lane.right = (k + 1 < n).then(|| start + k + 1);
                }
            }
            self.segment_lanes.insert(sid, ids);
        }

        for (ni, node) in roads.nodes.iter().enumerate() {
            let nid = NodeId(ni as u32);
            let approaches: Vec<SegmentId> = roads.streets_at(nid).collect();
            if approaches.is_empty() {
                continue;
            }
            let mut incoming: Vec<(LaneId, SegmentId)> = Vec::new();
            let mut outgoing: Vec<(LaneId, SegmentId)> = Vec::new();
            for &sid in &approaches {
                let seg = roads.segment(sid);
                // A piece too short for lanes (under half a metre) has none.
                for &lid in self.segment_lanes.get(&sid).into_iter().flatten() {
                    if let LaneKind::Road { forward, .. } = self.lanes[lid as usize].kind {
                        let ends_here = if forward { seg.b == nid } else { seg.a == nid };
                        if ends_here {
                            incoming.push((lid, sid));
                        } else {
                            outgoing.push((lid, sid));
                        }
                    }
                }
            }
            let radius = self.node_radius[ni];
            let mut connectors = Vec::new();
            let lane_index = |l: &Lane| match l.kind {
                LaneKind::Road { index, .. } => index as usize,
                _ => 0,
            };
            let width = |set: &[(LaneId, SegmentId)], seg: SegmentId| set.iter().filter(|&&(_, s)| s == seg).count();
            for &(inc, inc_seg) in &incoming {
                let approach = approaches.iter().position(|&s| s == inc_seg).unwrap_or(0);
                let (i, n) = (lane_index(&self.lanes[inc as usize]), width(&incoming, inc_seg));
                let d_in = self.lanes[inc as usize].end_dir();
                // Exits from this approach, leftmost first, with their turn.
                let mut exits: Vec<(SegmentId, f32)> = Vec::new();
                for &(out, out_seg) in &outgoing {
                    if (out_seg == inc_seg && approaches.len() > 1) || exits.iter().any(|&(s, _)| s == out_seg) {
                        continue; // U-turns only at dead ends
                    }
                    let d_out = self.lanes[out as usize].start_dir();
                    exits.push((out_seg, d_in.perp_dot(d_out).atan2(d_in.dot(d_out))));
                }
                exits.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
                let turn = |angle: f32| if approaches.len() <= 2 || angle.abs() < 0.5 { 0 } else if angle < 0.0 { -1 } else { 1 };
                // A connector for every movement into one lane of each exit:
                // a left turn keeps its place counted from the left, a right
                // turn from the right, straight on fans n lanes over m. The
                // lane arrows then permit some of them.
                for &(out, out_seg) in &outgoing {
                    let Some(&(_, angle)) = exits.iter().find(|&&(s, _)| s == out_seg) else { continue };
                    let (j, m) = (lane_index(&self.lanes[out as usize]), width(&outgoing, out_seg));
                    let ok = match turn(angle) {
                        -1 => j == i.min(m - 1),
                        1 => j == m - 1 - (n - 1 - i).min(m - 1),
                        _ => {
                            let lo = i * m / n.max(1);
                            let hi = ((i + 1) * m).div_ceil(n.max(1)).saturating_sub(1).max(lo);
                            (lo..=hi).contains(&j)
                        }
                    };
                    if ok {
                        let id = self.push_connector(inc, out, nid, approach, radius, approaches.len());
                        self.lanes[id as usize].turn = turn(angle);
                        self.lanes[id as usize].turnaround = out_seg == inc_seg;
                        connectors.push((id, approach));
                    }
                }
                // [r2-traffic] A U-turn from the leftmost lane of a wide
                // two-way street (2+ lanes each way) at a real junction,
                // swinging wide into the kerb lane going back. Driven as a
                // left turn (its phase, its arrow, giving way to oncoming
                // traffic); permitted only at a signalled junction from a
                // lane with a left arrow (`refresh_uturns`). Drivers used
                // to go out and back down the nearest dead end instead (K22,
                // K49, K52).
                if i == 0 && approaches.len() >= 3 && uturn_street(roads, inc_seg) && approaches.iter().all(|&s| !roads.segment(s).is_highway() && !roads.segment(s).ring) {
                    let back = outgoing.iter().filter(|&&(_, s)| s == inc_seg).map(|&(l, _)| l).max_by_key(|&l| lane_index(&self.lanes[l as usize]));
                    if let Some(out) = back {
                        let id = self.push_connector(inc, out, nid, approach, radius, approaches.len());
                        let c = &mut self.lanes[id as usize];
                        (c.turn, c.uturn, c.permitted) = (-1, true, false);
                        connectors.push((id, approach));
                    }
                }
            }
            // Lane arrows for every street arriving at a real junction (not
            // highway ramps: merges and diverges keep every lane open).
            if approaches.len() >= 3 && !approaches.iter().any(|&s| roads.segment(s).is_highway()) {
                for &sid in &approaches {
                    let mut lanes: Vec<LaneId> = incoming.iter().filter(|&&(_, s)| s == sid).map(|&(l, _)| l).collect();
                    if lanes.is_empty() {
                        continue;
                    }
                    lanes.sort_by_key(|&l| lane_index(&self.lanes[l as usize]));
                    let exits = self.exits_of(&lanes);
                    let arrows = default_arrows(lanes.len(), exits, None, None);
                    let idx = self.approaches.len() as u32;
                    for &l in &lanes {
                        self.lanes[l as usize].approach = idx;
                    }
                    self.approaches.push(Approach { node: nid, segment: sid, lanes, exits, arrows: Vec::new(), fixed: false, counts: [0.0; 3] });
                    self.set_arrows(idx as usize, arrows);
                }
            }
            let two_way = approaches.iter().filter(|&&s| roads.segment(s).lanes_fwd > 0 && roads.segment(s).lanes_bwd > 0).count();
            let feeding: Vec<usize> = (0..approaches.len()).filter(|&ap| incoming.iter().any(|&(_, s)| s == approaches[ap])).collect();
            // Direction of each approach away from the junction.
            let here = node.pos;
            let away: Vec<Vec2> = approaches
                .iter()
                .map(|&sid| {
                    let (a, b) = roads.segment_points(sid);
                    (if a.distance(here) < b.distance(here) { b - a } else { a - b }).normalize_or_zero()
                })
                .collect();
            let mut opposite: Vec<Option<usize>> = vec![None; approaches.len()];
            for &a in &feeding {
                let best = feeding.iter().copied().filter(|&b| b != a).min_by(|&x, &y| away[a].dot(away[x]).total_cmp(&away[a].dot(away[y])));
                if let Some(b) = best.filter(|&b| away[a].dot(away[b]) < -0.85) {
                    opposite[a] = Some(b);
                }
            }
            for a in 0..approaches.len() {
                if opposite[a].is_some_and(|b| opposite[b] != Some(a)) {
                    opposite[a] = None;
                }
            }
            // Mutual opposites share a phase; the rest go alone.
            let mut phases: Vec<Vec<usize>> = Vec::new();
            for &a in &feeding {
                if phases.iter().any(|g| g.contains(&a)) {
                    continue;
                }
                match opposite[a] {
                    Some(b) => phases.push(vec![a, b]),
                    None => phases.push(vec![a]),
                }
            }
            // One incoming approach (or one opposed pair) has nobody to yield to.
            let signalized = default_signalized(approaches.len(), two_way, phases.len());
            self.node_intersection[ni] = self.intersections.len();
            let incoming = incoming.iter().map(|&(l, s)| (l, approaches.iter().position(|&a| a == s).unwrap_or(0))).collect();
            let greens = vec![GREEN_SECONDS; phases.len()];
            self.intersections.push(Intersection { node: nid, radius, approaches: approaches.len(), phases, phase: 0, opposite, arrow: 0.0, left_turns: 0, left_wait_sum: 0.0, left_wait_max: 0.0, timer: 0.0, connectors, incoming, signalized, greens, arrow_mode: Default::default(), arrow_len: ARROW_SECONDS, ruled: false });
        }
        for id in 0..self.lanes.len() {
            for k in 0..self.lanes[id].next.len() {
                let next = self.lanes[id].next[k] as usize;
                self.lanes[next].prev.push(id as LaneId);
            }
        }
        self.ramp_profiles(roads);
        self.apply_roundabouts(roads);
        self.apply_signals();
        self.refresh_uturns();
    }

    pub(super) fn push_connector(&mut self, inc: LaneId, out: LaneId, nid: NodeId, approach: usize, radius: f32, approaches: usize) -> LaneId {
        let (p_in, d_in) = (self.lanes[inc as usize].end(), self.lanes[inc as usize].end_dir());
        let (p_out, d_out) = (self.lanes[out as usize].start(), self.lanes[out as usize].start_dir());
        let pts = connector_curve(p_in, d_in, p_out, d_out, radius);
        let id = self.lanes.len() as LaneId;
        let through = self.lanes[inc as usize].speed_limit.min(self.lanes[out as usize].speed_limit);
        // Along a highway (past a ramp too) traffic keeps its speed.
        let speed = if through > ROAD_SPEED && d_in.dot(d_out) > 0.9 {
            through
        } else if approaches <= 2 && d_in.dot(d_out) > 0.7 {
            ROAD_SPEED
        } else {
            CONNECTOR_SPEED
        };
        self.lanes.push(Lane::new(pts, LaneKind::Connector { node: nid, approach }, nid, speed));
        self.lanes[id as usize].next.push(out);
        self.lanes[inc as usize].next.push(id);
        id
    }

    /// Give an approach new lane arrows: its lanes' `arrows` and which of
    /// their connectors are permitted.
    /// The lane running the other way beside a single-lane carriageway:
    /// the opposing direction's leftmost lane, beside `lane`'s leftmost.
    pub fn opposing(&self, lane: &Lane) -> Option<LaneId> {
        let LaneKind::Road { segment, forward, index } = lane.kind else { return None };
        if index != 0 {
            return None;
        }
        self.segment_lanes.get(&segment)?.iter().copied().find(|&l| matches!(self.lanes[l as usize].kind, LaneKind::Road { forward: f, index: 0, .. } if f != forward))
    }

    /// The turns the connectors out of `lanes` (an approach) make, as a
    /// `LEFT | STRAIGHT | RIGHT` mask.
    pub fn exits_of(&self, lanes: &[LaneId]) -> u8 {
        // A U-turn rides on the left arrow; it never asks for one.
        lanes.iter().flat_map(|&l| self.lanes[l as usize].next.iter()).filter(|&&c| !self.lanes[c as usize].uturn).fold(0u8, |acc, &c| acc | turn_bit(self.lanes[c as usize].turn))
    }

    /// [r2-traffic] May a car take U-turn connector `c` now: its junction
    /// is signalled and the lane feeding it keeps a left arrow.
    fn uturn_open(&self, c: LaneId) -> bool {
        let lane = &self.lanes[c as usize];
        let LaneKind::Connector { node, .. } = lane.kind else { return false };
        let signalled = self.node_intersection.get(node.0 as usize).and_then(|&k| self.intersections.get(k)).is_some_and(|i| i.signalized);
        signalled && !self.rings.nodes.contains_key(&node) && lane.prev.iter().any(|&p| self.lanes[p as usize].arrows & LEFT != 0)
    }

    /// [r2-traffic] Open or close every U-turn connector by its junction's
    /// signals and its lane's arrows (after a rebuild, a junction rule or
    /// new arrows). True when any changed.
    pub fn refresh_uturns(&mut self) -> bool {
        let mut changed = false;
        for c in 0..self.lanes.len() {
            if self.lanes[c].uturn {
                let open = self.uturn_open(c as LaneId);
                changed |= self.lanes[c].permitted != open;
                self.lanes[c].permitted = open;
            }
        }
        changed
    }

    pub fn set_arrows(&mut self, approach: usize, arrows: Vec<u8>) {
        let Some(ap) = self.approaches.get_mut(approach) else { return };
        if ap.arrows == arrows {
            return;
        }
        ap.arrows = arrows.clone();
        let lanes = ap.lanes.clone();
        for (k, &l) in lanes.iter().enumerate() {
            let a = arrows.get(k).copied().unwrap_or(0);
            self.lanes[l as usize].arrows = a;
            for c in self.lanes[l as usize].next.clone() {
                let bit = turn_bit(self.lanes[c as usize].turn);
                self.lanes[c as usize].permitted = a & bit != 0 && (!self.lanes[c as usize].uturn || self.uturn_open(c));
            }
        }
        self.arrows_version += 1;
    }
}
