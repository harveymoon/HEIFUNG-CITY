//! The pedestrian network: pavements on both sides of every street, path
//! centres, crossings, a walkable setback loop around every usable lot, an
//! alley cross inside the Stacks, and gates between lots that share an
//! edge. Walkers route over this with A*, so people cut through blocks and
//! the shops they pass get walk-in trade.
//!
//! Streets are crossed only at crossings (`Crossing`): a crosswalk across
//! every arm of a junction of three or more streets, set back where
//! `nd_procgen::street` paints its zebra (signalised with the junction, see
//! `traffic/crosswalk.rs`); a zebra where a footpath meets a street and
//! every 120 m or so along a long block (`midblock_crossings`); and the end
//! of a dead end, unmarked. Pavements meet round the corners of a junction,
//! never through its middle. Opposite lot fronts are also joined by `Jay`
//! edges, which only jaywalkers' routes may use (`route_with`).

use crate::geom::{self, Grid};
use crate::roads::{NodeId, RoadKind, RoadNetwork, SegmentId};
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
/// Middle of a junction's zebras beyond its widest half width: the node
/// radius (widest half width + 1) plus 2.3 m, where `nd_procgen::street`
/// paints them.
pub const CROSSWALK_BACK: f32 = 3.3;
/// A roundabout arm's zebra is further out, past the splitter island.
pub const RING_CROSSWALK_BACK: f32 = 7.5;
/// Half the depth of a zebra along the street.
pub const CROSSWALK_HALF: f32 = 1.2;
/// The longest straight a junction corner's rounded kerb takes along each
/// arm (`nd_procgen::street`): the nose of a sharp fork is rounded tightly
/// rather than over tens of metres.
pub const CORNER_TANGENT_MAX: f32 = 4.0;
/// A run of street between junctions longer than this gets mid-block
/// zebras, about one every `MIDBLOCK_EVERY` metres.
pub const MIDBLOCK_MIN: f32 = 170.0;
pub const MIDBLOCK_EVERY: f32 = 120.0;
/// Jaywalking edges join lot fronts across a street at most this far apart
/// along it, and cost this many metres extra (the look and the dash).
const JAY_REACH: f32 = 12.0;
const JAY_PENALTY: f32 = 4.0;
/// [tools] What crossing at grade under a footbridge costs a walker over
/// its length, in metres: the wait for a gap or the light. Walkers take the
/// bridge when its stairs cost no more than this extra (`route_nodes_with`).
pub const BRIDGED_GRADE_PENALTY: f32 = 30.0;
/// `WalkEdge::cross` of an edge that crosses no street.
pub const NO_CROSSING: u32 = u32::MAX;

#[derive(Clone, Debug)]
pub struct WalkNode {
    pub pos: Vec2,
    /// Lot this node lies inside, for lot-interior edges.
    pub lot: Option<LotId>,
    /// Lots whose frontage this node passes, for footfall.
    pub near: Vec<LotId>,
    /// [tools] Height above the ground, metres: 0 at grade, the walkway's
    /// height on a raised walkway (a footbridge, later a skywalk).
    pub level: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct WalkEdge {
    pub to: u32,
    pub len: f32,
    /// Inside a lot (alley, setback, gate) rather than on a public street.
    pub interior: bool,
    /// A park path: pleasant, so routes prefer it a little.
    pub park: bool,
    /// Index into `WalkGraph::crossings` when the edge crosses a street.
    pub cross: u32,
    /// [tools] Part of a raised walkway (its deck or its stairs): off the
    /// street, so nobody on it holds up traffic. Footbridges are the first;
    /// skywalks between buildings can join them (`WalkGraph::raised`).
    pub raised: bool,
}

/// What kind of place a walker crosses a street.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossingKind {
    /// Across an arm of a junction of three or more streets: signalised
    /// when the junction is, a zebra otherwise.
    Junction,
    /// A painted zebra: a footpath over a street, or mid-block.
    Zebra,
    /// Unmarked but fair: the end of a dead end, a roundabout's own lanes.
    Informal,
    /// Straight over between opposite lot fronts: jaywalking.
    Jay,
}

/// One place a walker crosses a street: the edge between nodes `a` and `b`.
#[derive(Clone, Debug)]
pub struct Crossing {
    pub a: u32,
    pub b: u32,
    /// The street crossed.
    pub segment: SegmentId,
    /// The junction it belongs to, for a junction's crosswalk.
    pub node: Option<NodeId>,
    pub kind: CrossingKind,
    /// Its middle, on the street's centre line.
    pub mid: Vec2,
    /// [tools] A push-button signal: drivers get a red only after a walker
    /// presses the button (`traffic::crosswalk`, `world/pedestrians.rs`).
    pub button: bool,
    /// [tools] A footbridge spans it: walkers prefer the bridge.
    pub bridged: bool,
    /// [footbridge] The footbridge over it replaced it: the at-grade edge
    /// is gone from the walk graph, nothing is painted, no walk light, and
    /// drivers do not give way here (someone still out on it, on a route
    /// planned before, is watched like anyone in the road).
    pub closed: bool,
}

impl Crossing {
    /// Somewhere anyone may cross (minding the lights, if any).
    pub fn lawful(&self) -> bool {
        self.kind != CrossingKind::Jay
    }

    /// Painted on the road (a junction's crosswalk or a zebra): drivers give
    /// way to people on it. Elsewhere (a dead end, a roundabout's own lanes,
    /// straight over mid-block) nothing is painted and nobody stops: the
    /// walker finds a gap, at their own risk.
    pub fn marked(&self) -> bool {
        matches!(self.kind, CrossingKind::Junction | CrossingKind::Zebra) && !self.closed
    }
}

#[derive(Clone, Debug, Default)]
pub struct WalkGraph {
    pub nodes: Vec<WalkNode>,
    pub adj: Vec<Vec<WalkEdge>>,
    pub version: u64,
    /// Park benches: node, lot, and the direction a sitter faces.
    pub benches: Vec<(u32, LotId, f32)>,
    /// Every crossing; `WalkEdge::cross` indexes it.
    pub crossings: Vec<Crossing>,
    /// Grid over `nodes`, built at the end of `build`. `nearest` falls back
    /// to a scan if the node list has grown since.
    index: Grid,
    /// Crossing by the exact positions of its two ends, either way round,
    /// so a walker's path (a list of node positions) can find its crossings.
    cross_at: HashMap<[u32; 4], u32>,
    /// Indices into `benches` by lot, built with them, so a park's seats
    /// are counted without a scan of every bench in town.
    bench_lots: HashMap<LotId, Vec<u32>>,
    /// [tools] Every raised walkway edge once (lower node first), for
    /// drawing and for how high a walker on one stands.
    pub raised: Vec<(u32, u32)>,
    /// [tools] The footbridges: one per bridged crossing.
    pub footbridges: Vec<Footbridge>,
    /// [tools] Level of each raised node by its exact position.
    raised_at: HashMap<(u32, u32), f32>,
}

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
    /// Walk nodes: the two feet and the two tops.
    pub nodes: [u32; 4],
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

/// Live streets at a node.
fn street_arms(roads: &RoadNetwork, n: NodeId) -> usize {
    roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive && roads.segment(s).is_street()).count()
}

/// Widest half width of the streets at a node.
fn widest(roads: &RoadNetwork, n: NodeId) -> f32 {
    roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive && roads.segment(s).is_street()).map(|&s| roads.segment(s).half_width()).fold(0.0, f32::max)
}

/// A junction of three or more streets: its arms get crosswalks.
pub fn is_junction(roads: &RoadNetwork, n: NodeId) -> bool {
    street_arms(roads, n) >= 3
}

/// Distance from a junction's centre to the middle of the crosswalk
/// across `seg` (as `nd_procgen::street` paints it).
pub fn crosswalk_setback(roads: &RoadNetwork, node: NodeId, seg: SegmentId) -> f32 {
    let w = widest(roads, node);
    let usual = if roads.ring_of_node(node).is_some() && roads.ring_of_segment(seg).is_none() { w + RING_CROSSWALK_BACK } else { w + CROSSWALK_BACK };
    usual.max(acute_setback(roads, node, seg))
}

/// [review] Where `seg` makes a sharp corner (under about 80 degrees) with
/// a neighbouring street at `node`, the two kerb lines meet far out from
/// the node and the rounded kerb (`nd_procgen::street`, radius 3 to 6 m)
/// begins further out still. A zebra at the usual set-back ended on the
/// asphalt between the two arms, with no kerb to drop and the pavement's
/// end in the other street. Out to where it meets the corner's kerb.
fn acute_setback(roads: &RoadNetwork, node: NodeId, seg: SegmentId) -> f32 {
    let here = roads.node(node).pos;
    let away = |s: SegmentId| {
        let (a, b) = roads.segment_points(s);
        (if a.distance(here) < b.distance(here) { b - a } else { a - b }).normalize_or_zero()
    };
    let me = roads.segment(seg);
    let (d1, h1) = (away(seg), me.half_width());
    let mut need: f32 = 0.0;
    for &s in &roads.node(node).segments {
        let o = roads.segment(s);
        if s == seg || !o.alive || !o.is_street() {
            continue;
        }
        let cos = d1.dot(away(s)).clamp(-1.0, 1.0);
        let theta = cos.acos();
        if !(0.05..80f32.to_radians()).contains(&theta) {
            continue;
        }
        let h2 = o.half_width();
        // Along `seg`, where its kerb line meets the neighbour's.
        let meet = (h2 + h1 * cos) / theta.sin();
        // The rounded kerb's run along the arm past that point (a sharp
        // fork's nose is rounded tightly, `CORNER_TANGENT_MAX`): the zebra
        // lies past it, on the straight kerb, where the kerb drops square
        // across it ([road-polish]: it used to sit half on the curve, whose
        // kerb then stood full height across one corner of the zebra).
        let r = (0.6 * h1.min(h2)).clamp(3.0, 6.0);
        let round = (r * ((std::f32::consts::PI - theta) * 0.5).tan()).min(CORNER_TANGENT_MAX);
        need = need.max(meet + round + CROSSWALK_HALF + 0.3);
    }
    need
}

/// Mid-block zebras on long runs of street, as `(segment, t from a to b)`.
/// A run is a chain of street segments through plain bends (nodes where
/// two streets and nothing else meet); highways get none, and nor do
/// tunnels: nobody crosses a carriageway in the bore, they walk to a portal
/// or a junction (a zebra there was also drawn on the surface above).
pub fn midblock_crossings(roads: &RoadNetwork) -> Vec<(SegmentId, f32)> {
    let plain = |n: NodeId| roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive).count() == 2 && street_arms(roads, n) == 2;
    let ok = |s: SegmentId| {
        let sg = roads.segment(s);
        sg.alive && sg.is_street() && !sg.fronts_nothing()
    };
    let mut seen = vec![false; roads.segments.len()];
    let mut out = Vec::new();
    for (sid, _) in roads.alive_segments() {
        if seen[sid.0 as usize] || !ok(sid) {
            continue;
        }
        // Back to the start of the run.
        let (mut first, mut from) = (sid, roads.segment(sid).a);
        for _ in 0..roads.segments.len() {
            if !plain(from) {
                break;
            }
            let Some(o) = roads.node(from).segments.iter().copied().find(|&s| s != first && roads.segment(s).alive) else { break };
            if o == sid || !ok(o) {
                break;
            }
            let os = roads.segment(o);
            from = if os.a == from { os.b } else { os.a };
            first = o;
        }
        // Forward along it.
        let mut chain: Vec<(SegmentId, bool, f32)> = Vec::new();
        let mut cur = first;
        loop {
            if seen[cur.0 as usize] {
                break;
            }
            seen[cur.0 as usize] = true;
            let s = roads.segment(cur);
            let (pa, pb) = roads.segment_points(cur);
            let fwd = s.a == from;
            chain.push((cur, fwd, pa.distance(pb)));
            let to = if fwd { s.b } else { s.a };
            if !plain(to) {
                break;
            }
            let Some(next) = roads.node(to).segments.iter().copied().find(|&x| x != cur && roads.segment(x).alive) else { break };
            if !ok(next) {
                break;
            }
            cur = next;
            from = to;
        }
        let total: f32 = chain.iter().map(|c| c.2).sum();
        if total < MIDBLOCK_MIN {
            continue;
        }
        let k = ((total / MIDBLOCK_EVERY).round() as usize).saturating_sub(1).max(1);
        for j in 1..=k {
            let target = total * j as f32 / (k + 1) as f32;
            let mut acc = 0.0;
            for &(s, fwd, len) in &chain {
                if acc + len >= target {
                    let u = ((target - acc) / len.max(1e-3)).clamp(0.05, 0.95);
                    out.push((s, if fwd { u } else { 1.0 - u }));
                    break;
                }
                acc += len;
            }
        }
    }
    out
}

fn pos_key(p: Vec2, q: Vec2) -> [u32; 4] {
    [p.x.to_bits(), p.y.to_bits(), q.x.to_bits(), q.y.to_bits()]
}

/// A stop along one side of a street: a lot front or a mid-block zebra.
enum Stop {
    Lot(LotId),
    Zebra(usize),
}

type End = (u32, SegmentId, i8);

impl WalkGraph {
    fn add_node(&mut self, pos: Vec2, lot: Option<LotId>) -> u32 {
        self.nodes.push(WalkNode { pos, lot, near: Vec::new(), level: 0.0 });
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
        self.adj[a as usize].push(WalkEdge { to: b, len, interior, park, cross: NO_CROSSING, raised: false });
        self.adj[b as usize].push(WalkEdge { to: a, len, interior, park, cross: NO_CROSSING, raised: false });
    }

    /// Join two pavement nodes across a street.
    fn link_cross(&mut self, a: u32, b: u32, segment: SegmentId, node: Option<NodeId>, kind: CrossingKind, roads: &RoadNetwork) {
        if a == b {
            return;
        }
        let (pa, pb) = (self.nodes[a as usize].pos, self.nodes[b as usize].pos);
        // The middle on the street's centre line.
        let (s0, s1) = roads.segment_points(segment);
        let mid = geom::project_on_segment((pa + pb) * 0.5, s0, s1).1;
        let k = self.crossings.len() as u32;
        self.crossings.push(Crossing { a, b, segment, node, kind, mid, button: false, bridged: false, closed: false });
        let len = pa.distance(pb).max(0.05);
        self.adj[a as usize].push(WalkEdge { to: b, len, interior: false, park: false, cross: k, raised: false });
        self.adj[b as usize].push(WalkEdge { to: a, len, interior: false, park: false, cross: k, raised: false });
    }

    /// The crossing a walker takes going from `p` to `q`, two consecutive
    /// points of a route (node positions, exactly).
    pub fn crossing_between(&self, p: Vec2, q: Vec2) -> Option<u32> {
        if self.cross_at.is_empty() {
            return None;
        }
        self.cross_at.get(&pos_key(p, q)).copied()
    }

    /// A junction of three or more streets: a crosswalk across every arm
    /// (none over a highway), and the pavements of neighbouring arms meet
    /// round the corner between them, with a corner node where the arms
    /// are far apart so nobody cuts through the middle.
    fn junction(&mut self, roads: &RoadNetwork, nid: NodeId, centre: Vec2, e: &[End]) {
        struct Arm {
            seg: SegmentId,
            ang: f32,
            ccw: u32,
            cw: u32,
            street: bool,
        }
        let mut segs: Vec<SegmentId> = Vec::new();
        for x in e {
            if !segs.contains(&x.1) {
                segs.push(x.1);
            }
        }
        let mut arms: Vec<Arm> = Vec::new();
        for s in segs {
            let sg = roads.segment(s);
            let (a, b) = roads.segment_points(s);
            let away = (if sg.a == nid { b - a } else { a - b }).normalize_or_zero();
            let ends: Vec<u32> = e.iter().filter(|x| x.1 == s).map(|x| x.0).collect();
            let side = |n: &u32| away.perp_dot(self.nodes[*n as usize].pos - centre);
            let ccw = *ends.iter().max_by(|x, y| side(x).total_cmp(&side(y))).unwrap();
            let cw = *ends.iter().min_by(|x, y| side(x).total_cmp(&side(y))).unwrap();
            arms.push(Arm { seg: s, ang: away.y.atan2(away.x), ccw, cw, street: sg.is_street() });
        }
        arms.sort_by(|x, y| x.ang.total_cmp(&y.ang).then(x.seg.cmp(&y.seg)));
        let ring = roads.ring_of_node(nid).is_some();
        for arm in &arms {
            if arm.street && arm.ccw != arm.cw && !roads.segment(arm.seg).fronts_nothing() {
                let kind = if ring && roads.ring_of_segment(arm.seg).is_some() { CrossingKind::Informal } else { CrossingKind::Junction };
                self.link_cross(arm.cw, arm.ccw, arm.seg, Some(nid), kind, roads);
            }
        }
        let reach = widest(roads, nid) + SIDEWALK * 0.55 + 0.4;
        let n = arms.len();
        for i in 0..n {
            let j = (i + 1) % n;
            let gap = (arms[j].ang - arms[i].ang).rem_euclid(std::f32::consts::TAU);
            let gap = if gap <= 1e-4 { std::f32::consts::TAU } else { gap };
            if gap > 2.6 {
                // A wide corner: go round it, not across the junction.
                let bis = Vec2::from_angle(arms[i].ang + gap * 0.5);
                let c = self.add_node(centre + bis * reach, None);
                self.link(arms[i].ccw, c, false);
                self.link(c, arms[j].cw, false);
            } else {
                self.link(arms[i].ccw, arms[j].cw, false);
            }
        }
    }

    /// Pair each end of the first segment in `among` with the nearest end
    /// of the other; false when there is no other.
    fn pair_ends(&mut self, among: &[End]) -> bool {
        let Some(first) = among.first() else { return false };
        let first_seg = first.1;
        let (mine, theirs): (Vec<End>, Vec<End>) = among.iter().partition(|x| x.1 == first_seg);
        for (na, _, _) in mine {
            let p = self.nodes[na as usize].pos;
            if let Some((nb, _, _)) = theirs.iter().copied().min_by(|x, y| self.nodes[x.0 as usize].pos.distance(p).total_cmp(&self.nodes[y.0 as usize].pos.distance(p))) {
                self.link(na, nb, false);
            }
        }
        !theirs.is_empty()
    }

    fn hub(&mut self, at: Vec2, e: &[End]) {
        let hub = self.add_node(at, None);
        for (n, _, _) in e {
            self.link(*n, hub, false);
        }
    }

    pub fn build(roads: &RoadNetwork, lots: &SlotMap<LotId, Lot>, land_value: &LandValue, version: u64) -> Self {
        let mut g = WalkGraph { version, ..Default::default() };

        // ── streets and paths ────────────────────────────────────────────
        // Endpoints per road node: (node index, segment, side).
        let mut ends: Vec<Vec<End>> = vec![Vec::new(); roads.nodes.len()];
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
        let mut zebras: HashMap<SegmentId, Vec<f32>> = HashMap::new();
        for (s, t) in midblock_crossings(roads) {
            zebras.entry(s).or_default().push(t);
        }
        // How far back from a node a street's pavements end: at the
        // crosswalk of a junction, at the node elsewhere.
        let setback = |n: NodeId, s: SegmentId| if is_junction(roads, n) { crosswalk_setback(roads, n, s) } else { 0.0 };
        // A footpath meeting streets ends on their pavement.
        let path_setback = |n: NodeId| if street_arms(roads, n) > 0 { widest(roads, n) + SIDEWALK * 0.55 } else { 0.0 };
        for (sid, seg) in roads.alive_segments() {
            // No one walks in a road tunnel, nor along a highway or its
            // slip roads: they have hard shoulders, not pavements.
            if seg.fronts_nothing() {
                continue;
            }
            let (a, b) = roads.segment_points(sid);
            let len = a.distance(b).max(1e-3);
            let d = (b - a) / len;
            let right = Vec2::new(-d.y, d.x);
            let fronts = fronting.get(&sid).map(Vec::as_slice).unwrap_or(&[]);
            match seg.kind {
                RoadKind::Street => {
                    let off = seg.half_width() + SIDEWALK * 0.55;
                    let (mut sa, mut sb) = (setback(seg.a, sid), setback(seg.b, sid));
                    if sa + sb > len * 0.8 {
                        let k = len * 0.8 / (sa + sb);
                        sa *= k;
                        sb *= k;
                    }
                    let (ta, tb) = (sa / len + 0.01, 1.0 - sb / len - 0.01);
                    let mids: Vec<f32> = zebras.get(&sid).cloned().unwrap_or_default();
                    let mut zebra_nodes: Vec<[u32; 2]> = vec![[0; 2]; mids.len()];
                    let mut lot_nodes: [Vec<(f32, u32)>; 2] = [Vec::new(), Vec::new()];
                    for (si, side) in [-1i8, 1].into_iter().enumerate() {
                        let o = right * off * side as f32;
                        // Lots fronting this side, and the zebras, ordered along the segment.
                        let mut stops: Vec<(f32, Stop)> = fronts
                            .iter()
                            .filter(|(_, l)| ((l.entrance() - a).dot(right) >= 0.0) == (side > 0))
                            .map(|(id, l)| (l.front_t.clamp(0.02, 0.98).clamp(ta.min(0.5), tb.max(0.5)), Stop::Lot(*id)))
                            .collect();
                        stops.extend(mids.iter().enumerate().map(|(k, &t)| (t, Stop::Zebra(k))));
                        stops.sort_by(|x, y| x.0.total_cmp(&y.0));
                        let na = g.add_node(a + d * sa + o, None);
                        let mut prev = na;
                        for (t, stop) in stops {
                            let n = g.add_node(a + d * (t * len) + o, None);
                            match stop {
                                Stop::Lot(lid) => {
                                    g.nodes[n as usize].near.push(lid);
                                    front_nodes.insert(lid, n);
                                    lot_nodes[si].push((t, n));
                                }
                                Stop::Zebra(k) => zebra_nodes[k][si] = n,
                            }
                            g.link(prev, n, false);
                            street_nodes.push(n);
                            prev = n;
                        }
                        let nb = g.add_node(b - d * sb + o, None);
                        g.link(prev, nb, false);
                        ends[seg.a.0 as usize].push((na, sid, side));
                        ends[seg.b.0 as usize].push((nb, sid, side));
                        street_nodes.push(na);
                        street_nodes.push(nb);
                    }
                    for z in zebra_nodes {
                        g.link_cross(z[0], z[1], sid, None, CrossingKind::Zebra, roads);
                    }
                    // Jaywalkers' short cuts: straight over to the lot
                    // front opposite.
                    if !seg.fronts_nothing() {
                        for &(t, n) in &lot_nodes[0] {
                            let best = lot_nodes[1].iter().min_by(|x, y| (x.0 - t).abs().total_cmp(&(y.0 - t).abs()));
                            if let Some(&(_, n2)) = best.filter(|x| (x.0 - t).abs() * len <= JAY_REACH) {
                                g.link_cross(n, n2, sid, None, CrossingKind::Jay, roads);
                            }
                        }
                    }
                }
                RoadKind::Rail => {}
                RoadKind::Path => {
                    // Homes may line a path: a node opposite each of them.
                    let (sa, sb) = (path_setback(seg.a).min(len * 0.4), path_setback(seg.b).min(len * 0.4));
                    let mut stops: Vec<(f32, LotId)> = fronts.iter().map(|(id, l)| (l.front_t.clamp(0.02, 0.98), *id)).collect();
                    stops.sort_by(|x, y| x.0.total_cmp(&y.0));
                    let na = g.add_node(a + d * sa, None);
                    let mut prev = na;
                    for (t, lid) in stops {
                        let n = g.add_node(a + d * (t * len), None);
                        g.nodes[n as usize].near.push(lid);
                        g.link(prev, n, false);
                        front_nodes.insert(lid, n);
                        street_nodes.push(n);
                        prev = n;
                    }
                    let nb = g.add_node(b - d * sb, None);
                    g.link(prev, nb, false);
                    ends[seg.a.0 as usize].push((na, sid, 0));
                    ends[seg.b.0 as usize].push((nb, sid, 0));
                    street_nodes.push(na);
                    street_nodes.push(nb);
                }
            }
        }
        // Joins at road nodes: plain bends link matching sides directly;
        // junctions get crosswalks and corners; a footpath meeting a street
        // gets a zebra; a dead end is crossed at its end.
        for (ni, node) in roads.nodes.iter().enumerate() {
            let e = &ends[ni];
            if e.is_empty() {
                continue;
            }
            let nid = NodeId(ni as u32);
            let mut streets: Vec<SegmentId> = Vec::new();
            let mut paths = 0;
            let mut arms: Vec<SegmentId> = Vec::new();
            for x in e {
                if !arms.contains(&x.1) {
                    arms.push(x.1);
                    if roads.segment(x.1).is_street() {
                        streets.push(x.1);
                    } else {
                        paths += 1;
                    }
                }
            }
            if streets.len() >= 3 {
                g.junction(roads, nid, node.pos, e);
                continue;
            }
            if streets.is_empty() || (arms.len() == 2 && streets.len() == 2) || (arms.len() == 1 && paths == 1) {
                // Footpaths only, or a plain bend of two streets.
                if arms.len() == 2 && e.len() >= 2 {
                    if !g.pair_ends(e) {
                        g.hub(node.pos, e);
                    }
                } else {
                    g.hub(node.pos, e);
                }
                continue;
            }
            // One or two streets with footpaths, or a dead end.
            let on_street: Vec<End> = e.iter().copied().filter(|x| roads.segment(x.1).is_street()).collect();
            if streets.len() == 2 {
                g.pair_ends(&on_street);
            }
            let s0 = streets[0];
            let sides: Vec<u32> = on_street.iter().filter(|x| x.1 == s0).map(|x| x.0).collect();
            if sides.len() == 2 && !roads.segment(s0).fronts_nothing() {
                let kind = if streets.len() == 2 && paths > 0 { CrossingKind::Zebra } else { CrossingKind::Informal };
                g.link_cross(sides[0], sides[1], s0, Some(nid), kind, roads);
            }
            for x in e.iter().filter(|x| !roads.segment(x.1).is_street()) {
                let p = g.nodes[x.0 as usize].pos;
                if let Some(best) = on_street.iter().min_by(|u, v| g.nodes[u.0 as usize].pos.distance(p).total_cmp(&g.nodes[v.0 as usize].pos.distance(p))).map(|y| y.0) {
                    g.link(x.0, best, false);
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
        for (k, b) in g.benches.iter().enumerate() {
            g.bench_lots.entry(b.1).or_default().push(k as u32);
        }
        g.reindex();
        g
    }

    /// The node grid and the crossing lookup, after nodes were added.
    fn reindex(&mut self) {
        self.index = Grid::new(NODE_CELL, self.nodes.iter().map(|n| (n.pos, n.pos)));
        self.cross_at.clear();
        for (k, c) in self.crossings.iter().enumerate() {
            let (pa, pb) = (self.nodes[c.a as usize].pos, self.nodes[c.b as usize].pos);
            self.cross_at.insert(pos_key(pa, pb), k as u32);
            self.cross_at.insert(pos_key(pb, pa), k as u32);
        }
        self.raised_at = self.nodes.iter().filter(|n| n.level != 0.0).map(|n| ((n.pos.x.to_bits(), n.pos.y.to_bits()), n.level)).collect();
    }

    /// [tools] Put the Intersection editor's crosswalk upgrades on the
    /// crossings they were made for (matched by position and the street's
    /// direction): a push button, or a footbridge with its raised walkway.
    pub fn apply_upgrades(&mut self, rules: &[crate::junctions::CrossingRule], roads: &RoadNetwork) {
        use crate::junctions::CrossingUpgrade;
        if rules.is_empty() {
            return;
        }
        for k in 0..self.crossings.len() {
            let c = &self.crossings[k];
            if c.kind == CrossingKind::Jay || roads.try_segment(c.segment).is_none() {
                continue;
            }
            let (s0, s1) = roads.segment_points(c.segment);
            let dir = (s1 - s0).normalize_or_zero();
            let Some(rule) = rules.iter().find(|r| r.at.distance(c.mid) < 2.0 && r.dir.dot(dir).abs() > 0.9) else { continue };
            match rule.upgrade {
                CrossingUpgrade::None => {}
                CrossingUpgrade::Button => self.crossings[k].button = true,
                CrossingUpgrade::Footbridge => {
                    let along = Self::stair_dir(c, dir, roads);
                    self.add_footbridge(k as u32, along);
                    // [footbridge] The bridge replaces the crosswalk under
                    // it unless the player kept it: everyone crossing here
                    // climbs over, and nobody waits in the road.
                    if !rule.keep_crosswalk {
                        self.close_crossing(k as u32);
                    }
                }
            }
        }
        self.reindex();
    }

    /// A footbridge over crossing `k`: the walkway a little to one side of
    /// the crosswalk at `FOOTBRIDGE_LEVEL`, stairs from each end along the
    /// pavement (`along`), and the feet joined to the pavement both ways.
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
    fn stair_dir(c: &Crossing, dir: Vec2, roads: &RoadNetwork) -> Vec2 {
        match c.node {
            Some(n) if (c.mid - roads.node(n).pos).dot(dir) < 0.0 => -dir,
            _ => dir,
        }
    }

    fn add_footbridge(&mut self, k: u32, along: Vec2) {
        use crate::junctions::{FOOTBRIDGE_LEVEL, FOOTBRIDGE_RUN, FOOTBRIDGE_WIDTH};
        let c = self.crossings[k as usize].clone();
        let ends = [c.a, c.b];
        let mut nodes = [0u32; 4];
        let mut tops = [Vec2::ZERO; 2];
        let mut feet = [Vec2::ZERO; 2];
        for (i, &end) in ends.iter().enumerate() {
            let p = self.nodes[end as usize].pos;
            let top = p + along * (FOOTBRIDGE_WIDTH + 0.5);
            let foot = top + along * FOOTBRIDGE_RUN;
            let t = self.add_node(top, None);
            self.nodes[t as usize].level = FOOTBRIDGE_LEVEL;
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
        self.footbridges.push(Footbridge { crossing: k, top: tops, foot: feet, level: FOOTBRIDGE_LEVEL, width: FOOTBRIDGE_WIDTH, nodes: [nodes[0], nodes[2], nodes[1], nodes[3]] });
    }

    /// [footbridge] Take crossing `k`'s at-grade edge out of the walk graph
    /// (its footbridge stands in for it). The crossing itself stays listed,
    /// `closed`, so indices and the editor's view of it hold.
    fn close_crossing(&mut self, k: u32) {
        let Some(c) = self.crossings.get_mut(k as usize) else { return };
        c.closed = true;
        let (a, b) = (c.a as usize, c.b as usize);
        self.adj[a].retain(|e| e.cross != k);
        self.adj[b].retain(|e| e.cross != k);
    }

    /// Join two nodes by a raised walkway edge (a deck or a flight of
    /// stairs), its length the walk along the slope.
    fn link_raised(&mut self, a: u32, b: u32) {
        if a == b {
            return;
        }
        let (na, nb) = (&self.nodes[a as usize], &self.nodes[b as usize]);
        let len = (na.pos.distance(nb.pos).powi(2) + (na.level - nb.level).powi(2)).sqrt().max(0.05);
        let (lo, hi) = if na.level <= nb.level { (a, b) } else { (b, a) };
        self.adj[a as usize].push(WalkEdge { to: b, len, interior: false, park: false, cross: NO_CROSSING, raised: true });
        self.adj[b as usize].push(WalkEdge { to: a, len, interior: false, park: false, cross: NO_CROSSING, raised: true });
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

    /// Nearest node within `max_dist`; exact ties go to the lower index.
    pub fn nearest(&self, p: Vec2, max_dist: f32) -> Option<u32> {
        // Raised walkways are reached by their stairs, never snapped onto.
        let within = |i: usize| {
            let d = self.nodes[i].pos.distance(p);
            if d <= max_dist && self.nodes[i].level == 0.0 { d } else { f32::INFINITY }
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

    /// Nearest node within `r` by a box query: unlike [`nearest`](Self::nearest)
    /// it never searches further out when nothing is that close, so asking
    /// about open country is as cheap as asking about a street. Exact ties
    /// go to the lower index.
    pub fn nearest_within(&self, p: Vec2, r: f32) -> Option<u32> {
        if self.index.len() != self.nodes.len() {
            return self.nearest(p, r);
        }
        let mut best: Option<(f32, u32)> = None;
        for i in self.index.in_box(p - Vec2::splat(r), p + Vec2::splat(r)) {
            let d = self.nodes[i as usize].pos.distance(p);
            if d <= r && self.nodes[i as usize].level == 0.0 && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// A* between two nodes with the edge costs of [`PARK_FACTOR`] and
    /// [`INTERIOR_FACTOR`]. The heuristic is the straight-line distance
    /// scaled by the cheapest factor, so it never overestimates and the
    /// first time the goal pops it is optimal.
    pub fn route_nodes(&self, from: u32, to: u32) -> Option<Vec<u32>> {
        self.route_nodes_with(from, to, false)
    }

    /// [`route_nodes`](Self::route_nodes), over `Jay` edges too when
    /// `jaywalk` (each costs [`JAY_PENALTY`] metres more than its length).
    pub fn route_nodes_with(&self, from: u32, to: u32, jaywalk: bool) -> Option<Vec<u32>> {
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
                let mut cost = e.len * if e.park { PARK_FACTOR } else if e.interior { INTERIOR_FACTOR } else { 1.0 };
                if e.cross != NO_CROSSING && self.crossings[e.cross as usize].kind == CrossingKind::Jay {
                    if !jaywalk {
                        continue;
                    }
                    cost += JAY_PENALTY;
                }
                // [tools] At grade under a footbridge: the bridge is worth
                // a little climbing.
                if e.cross != NO_CROSSING && self.crossings[e.cross as usize].bridged {
                    cost += BRIDGED_GRADE_PENALTY;
                }
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
        self.route_with(from, to, false)
    }

    /// [`route`](Self::route), letting a jaywalker take `Jay` edges.
    pub fn route_with(&self, from: Vec2, to: Vec2, jaywalk: bool) -> Option<(Vec<Vec2>, Vec<u32>)> {
        let a = self.nearest(from, 60.0)?;
        let b = self.nearest(to, 60.0)?;
        let nodes = self.route_nodes_with(a, b, jaywalk)?;
        let pts = nodes.iter().map(|&i| self.nodes[i as usize].pos).collect();
        Some((pts, nodes))
    }

    /// Metres on foot from the nearest of `sources` to every node: one
    /// multi-source Dijkstra over the lawful edges (no jaywalking), plain
    /// lengths without the route preferences, `f32::INFINITY` where none of
    /// them can be reached. Used for how far a home is from the nearest
    /// park or night out (`LandValue::access_at`), so it costs one pass
    /// per kind of amenity however many there are.
    pub fn distances_from(&self, sources: &[u32]) -> Vec<f32> {
        #[derive(PartialEq)]
        struct Item(f32, u32);
        impl Eq for Item {}
        impl PartialOrd for Item {
            fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for Item {
            // Min-heap on distance; ties by node index so the order of
            // equal pops never depends on how the heap was filled.
            fn cmp(&self, o: &Self) -> std::cmp::Ordering {
                o.0.total_cmp(&self.0).then(o.1.cmp(&self.1))
            }
        }
        let mut dist = vec![f32::INFINITY; self.nodes.len()];
        let mut heap = BinaryHeap::new();
        for &s in sources {
            if (s as usize) < dist.len() && dist[s as usize] > 0.0 {
                dist[s as usize] = 0.0;
                heap.push(Item(0.0, s));
            }
        }
        while let Some(Item(d, u)) = heap.pop() {
            if d > dist[u as usize] {
                continue;
            }
            for e in &self.adj[u as usize] {
                if e.cross != NO_CROSSING && self.crossings[e.cross as usize].kind == CrossingKind::Jay {
                    continue;
                }
                let nd = d + e.len;
                if nd < dist[e.to as usize] {
                    dist[e.to as usize] = nd;
                    heap.push(Item(nd, e.to));
                }
            }
        }
        dist
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

    /// [review] At a sharp corner the crosswalk is set out past where the
    /// two arms' kerb lines meet (else it ends on the asphalt between them,
    /// and so does the pavement); a right-angled junction keeps the usual
    /// set-back.
    #[test]
    fn sharp_corners_set_their_crosswalks_out_to_the_kerb() {
        let mut roads = RoadNetwork::default();
        let a = 150f32.to_radians();
        roads.add_polyline(&[Vec2::ZERO, Vec2::new(120.0, 0.0)], 1, 1);
        roads.add_polyline(&[Vec2::ZERO, Vec2::from_angle(a) * 120.0], 1, 1);
        roads.add_polyline(&[Vec2::ZERO, Vec2::from_angle(-a) * 120.0], 1, 1);
        let node = roads.nodes.iter().position(|n| n.pos.length() < 0.1).map(|i| NodeId(i as u32)).unwrap();
        let hw = roads.segment(roads.node(node).segments[0]).half_width();
        let usual = hw + CROSSWALK_BACK;
        // Where two kerb lines 60 degrees apart meet, along either arm.
        let meet = (hw + hw * 0.5) / 60f32.to_radians().sin();
        for &s in &roads.node(node).segments {
            let (p, q) = roads.segment_points(s);
            let far = if p.length() > q.length() { p } else { q };
            let set = crosswalk_setback(&roads, node, s);
            if far.x > 0.0 {
                assert_eq!(set, usual, "the straight-on arm keeps the usual set-back");
            } else {
                assert!(set - CROSSWALK_HALF > meet, "an arm of the sharp corner: zebra from {} m, the kerb lines meet at {meet} m", set - CROSSWALK_HALF);
            }
        }
        let mut plus = RoadNetwork::default();
        plus.add_polyline(&[Vec2::new(-120.0, 0.0), Vec2::new(120.0, 0.0)], 1, 1);
        plus.add_polyline(&[Vec2::new(0.0, -120.0), Vec2::new(0.0, 120.0)], 1, 1);
        let node = plus.nodes.iter().position(|n| n.pos.length() < 0.1).map(|i| NodeId(i as u32)).unwrap();
        for &s in &plus.node(node).segments {
            assert_eq!(crosswalk_setback(&plus, node, s), usual, "a crossroads is unchanged");
        }
    }

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

    /// Proper intersection of two segments (not just touching at an end).
    fn crosses(p: Vec2, q: Vec2, a: Vec2, b: Vec2) -> bool {
        let d1 = (q - p).perp_dot(a - p);
        let d2 = (q - p).perp_dot(b - p);
        let d3 = (b - a).perp_dot(p - a);
        let d4 = (b - a).perp_dot(q - a);
        d1 * d2 < -1e-3 && d3 * d4 < -1e-3
    }

    /// A grid with a T, a dead end, a footpath over a street and one long
    /// street, zoned so lots line the pavements.
    fn crossing_town() -> (RoadNetwork, SlotMap<LotId, Lot>, WalkGraph) {
        let mut roads = RoadNetwork::default();
        for c in [-70.0f32, 70.0] {
            roads.add_polyline(&[Vec2::new(-300.0, c), Vec2::new(300.0, c)], 1, 1);
        }
        roads.add_polyline(&[Vec2::new(-70.0, -300.0), Vec2::new(-70.0, 300.0)], 1, 1);
        // A T onto the north street, and a dead end off it.
        roads.add_polyline(&[Vec2::new(70.0, 70.0), Vec2::new(70.0, 200.0)], 1, 1);
        // A footpath across the south street.
        roads.add_polyline_kind(&[Vec2::new(150.0, -150.0), Vec2::new(150.0, 10.0)], 0, 0, RoadKind::Path);
        let mut lots = SlotMap::with_key();
        for (x, y) in [(0.0f32, 0.0f32), (0.0, -140.0), (0.0, 140.0), (-160.0, 0.0), (200.0, 0.0)] {
            let zone = Zone { polygon: vec![Vec2::new(x - 60.0, y - 60.0), Vec2::new(x + 60.0, y - 60.0), Vec2::new(x + 60.0, y + 60.0), Vec2::new(x - 60.0, y + 60.0)], land_use: LandUse::Commercial };
            generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        }
        let lv = LandValue::new(400.0, 32.0);
        let g = WalkGraph::build(&roads, &lots, &lv, 1);
        (roads, lots, g)
    }

    #[test]
    fn walkers_cross_streets_only_at_crossings() {
        let (roads, _, g) = crossing_town();
        let streets: Vec<(Vec2, Vec2)> = roads.alive_segments().filter(|(_, s)| s.is_street()).map(|(sid, _)| roads.segment_points(sid)).collect();
        let mut kinds = HashMap::new();
        for c in &g.crossings {
            *kinds.entry(format!("{:?}", c.kind)).or_insert(0) += 1;
        }
        for (i, edges) in g.adj.iter().enumerate() {
            for e in edges.iter().filter(|e| e.cross == NO_CROSSING) {
                let (p, q) = (g.nodes[i].pos, g.nodes[e.to as usize].pos);
                for &(a, b) in &streets {
                    assert!(!crosses(p, q, a, b), "a plain walk edge {p:?}-{q:?} crosses the street {a:?}-{b:?}");
                }
            }
        }
        // Nobody walks through the middle of a junction.
        for (ni, node) in roads.nodes.iter().enumerate() {
            if !is_junction(&roads, NodeId(ni as u32)) {
                continue;
            }
            let r = widest(&roads, NodeId(ni as u32));
            for (i, edges) in g.adj.iter().enumerate() {
                for e in edges {
                    let (p, q) = (g.nodes[i].pos, g.nodes[e.to as usize].pos);
                    let d = geom::project_on_segment(node.pos, p, q).1.distance(node.pos);
                    assert!(d > r, "an edge passes {d} m from the middle of the junction at {:?}", node.pos);
                }
            }
        }
        for k in ["Junction", "Zebra", "Informal", "Jay"] {
            assert!(kinds.get(k).copied().unwrap_or(0) > 0, "some {k} crossings: {kinds:?}");
        }
        // The long streets get mid-block zebras, the 140 m blocks none.
        assert!(!midblock_crossings(&roads).is_empty());
        // Lawful routes never jaywalk; a jaywalker's may.
        let (from, to) = (Vec2::new(-20.0, -62.0), Vec2::new(-20.0, -78.0));
        let lawful = g.route(from, to).unwrap().1;
        assert!(lawful.windows(2).all(|w| g.crossing_between(g.nodes[w[0] as usize].pos, g.nodes[w[1] as usize].pos).is_none_or(|c| g.crossings[c as usize].lawful())));
        let bold = g.route_with(from, to, true).unwrap().1;
        assert!(bold.windows(2).any(|w| g.crossing_between(g.nodes[w[0] as usize].pos, g.nodes[w[1] as usize].pos).is_some_and(|c| !g.crossings[c as usize].lawful())), "straight over");
        assert!(bold.len() < lawful.len());
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

    #[test]
    fn distances_from_the_nearest_of_several_sources_along_the_graph() {
        let mut g = WalkGraph::default();
        // a - b - c - d in a line 10 m apart, and e on its own.
        let ids: Vec<u32> = [0.0, 10.0, 20.0, 30.0, 100.0].iter().map(|&x| g.add_node(Vec2::new(x, 0.0), None)).collect();
        g.link(ids[0], ids[1], false);
        g.link(ids[1], ids[2], false);
        g.link(ids[2], ids[3], true);
        let d = g.distances_from(&[ids[0], ids[3]]);
        assert_eq!(d[0], 0.0);
        assert!((d[1] - 10.0).abs() < 1e-4 && (d[2] - 10.0).abs() < 1e-4, "{d:?}");
        assert_eq!(d[3], 0.0);
        assert!(d[4].is_infinite(), "unreachable stays infinite");
        assert!(g.distances_from(&[]).iter().all(|x| x.is_infinite()));
        // A nearest lookup that finds nothing near does not wander off.
        g.index = Grid::new(NODE_CELL, g.nodes.iter().map(|n| (n.pos, n.pos)));
        assert_eq!(g.nearest_within(Vec2::new(12.0, 3.0), 5.0), Some(ids[1]));
        assert_eq!(g.nearest_within(Vec2::new(60.0, 0.0), 20.0), None);
    }
}
