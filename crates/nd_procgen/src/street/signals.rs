//! Traffic and pedestrian signal posts.

use super::*;

/// Where a signal lamp hangs over the end of a lane, for the renderer to
/// swap its colour at runtime.
#[derive(Debug)]
pub struct SignalPost {
    pub lane: LaneId,
    pub lamp: Vec3,
    pub heading: f32,
}

/// [pedestrians] A walk / don't walk lamp at one end of a signalised
/// crosswalk, facing the people at the other end: the renderer colours it
/// from `Traffic::peds` (`walk_shown(inter, approach)`).
#[derive(Debug)]
pub struct PedSignalPost {
    pub inter: usize,
    pub approach: usize,
    /// [footbridge] The middle of the crosswalk it serves.
    pub crosswalk: Vec2,
    /// Foot of the pole, at the kerb.
    pub pole: Vec2,
    pub lamp: Vec3,
    /// Direction the lamp faces (over the street), as an angle.
    pub heading: f32,
}

/// Height of a pedestrian signal head.
pub(super) const PED_HEAD_Y: f32 = 2.55;

pub(super) struct SignalGroup {
    /// The junction the approach feeds.
    pub(super) node: NodeId,
    /// [interchange] The approach.
    pub(super) seg: SegmentId,
    pub(super) pole: Vec2,
    pub(super) leftmost: Vec2,
    pub(super) heads: Vec<Vec2>,
    pub(super) heading: f32,
}

/// Road lanes feeding each connector lane, built once: the signal passes
/// used to scan every lane for every connector of every junction, and the
/// result came out of a `HashMap` in a different order each run.
pub(super) fn feeders(net: &LaneNetwork) -> BTreeMap<LaneId, Vec<LaneId>> {
    let mut map: BTreeMap<LaneId, Vec<LaneId>> = BTreeMap::new();
    for (lid, lane) in net.lanes.iter().enumerate() {
        if lane.is_connector() {
            continue;
        }
        for &next in &lane.next {
            map.entry(next).or_default().push(lid as LaneId);
        }
    }
    map
}

/// Approaches to signalised junctions grouped by segment and direction, in
/// segment order so the street mesh is the same every time it is built.
pub(super) fn signal_groups(roads: &RoadNetwork, net: &LaneNetwork, corners: &Corners) -> Vec<SignalGroup> {
    let feeders = feeders(net);
    let mut arms: HashMap<u32, Vec<Mouth>> = HashMap::new();
    let mut groups: BTreeMap<(u32, bool), (NodeId, Vec<(u8, Vec2, Vec2)>)> = BTreeMap::new();
    for inter in &net.intersections {
        if !inter.signalized {
            continue;
        }
        for &(conn, _) in &inter.connectors {
            for &lid in feeders.get(&conn).map(|v| v.as_slice()).unwrap_or(&[]) {
                let lane = &net.lanes[lid as usize];
                if let LaneKind::Road { segment, forward, index } = lane.kind {
                    let back = junction::extra_stop_back(roads, net, inter.node, segment);
                    let (p, d) = lane.point_at((lane.length - 0.6 - back).max(0.0));
                    let (_, entry) = groups.entry((segment.0, forward)).or_insert_with(|| (inter.node, Vec::new()));
                    if !entry.iter().any(|(i, _, _)| *i == index) {
                        entry.push((index, p, d));
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for ((seg, _), (node, lanes)) in groups.into_iter() {
        let Some((_, _, d)) = lanes.first() else { continue };
        let d = *d;
        let right = Vec2::new(-d.y, d.x);
        let rightmost = lanes.iter().max_by_key(|(i, _, _)| *i).unwrap();
        let leftmost = lanes.iter().min_by_key(|(i, _, _)| *i).unwrap();
        // On the pavement: behind the corner's kerb where the corner is
        // rounded off past it.
        let pole = behind_kerb(rightmost.1 + right * (LANE_WIDTH * 0.5 + SHOULDER + 0.45), corners.by_node.get(&node.0), 0.45);
        let pole = if pad_node(roads, node) { junction::place_pole(roads.node(node).pos, arms.entry(node.0).or_insert_with(|| mouths(roads, net, node)), SegmentId(seg), pole, 0.45) } else { pole };
        out.push(SignalGroup { node, seg: SegmentId(seg), pole, leftmost: leftmost.1 - right * (LANE_WIDTH * 0.2), heads: lanes.iter().map(|(_, p, _)| *p).collect(), heading: d.y.atan2(d.x) });
    }
    out
}

/// [pedestrians] Walk lamps for every arm of every signalised junction
/// (none across a highway), two per crosswalk, in junction and arm order.
/// The crosswalk sits where `nd_sim::walk` puts it, so the lamps stand at
/// the kerbs walkers wait at.
pub fn ped_signal_posts(roads: &RoadNetwork, net: &LaneNetwork) -> Vec<PedSignalPost> {
    ped_signal_posts_at(roads, net, &junction_corners(roads, net, &build_runs(roads)))
}

pub(super) fn ped_signal_posts_at(roads: &RoadNetwork, net: &LaneNetwork, corners: &Corners) -> Vec<PedSignalPost> {
    let mut out = Vec::new();
    for (k, inter) in net.intersections.iter().enumerate() {
        if !inter.signalized || roads.nodes.len() <= inter.node.0 as usize {
            continue;
        }
        let node = roads.node(inter.node);
        let arms: Vec<SegmentId> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
        let mouth_list = if pad_node(roads, inter.node) { mouths(roads, net, inter.node) } else { Vec::new() };
        for (ap, &sid) in arms.iter().enumerate() {
            let seg = roads.segment(sid);
            // No walk lights where nobody crosses (a ramp, a tunnel: the
            // walk graph has no crosswalk there either).
            if !junction::has_crosswalk(roads, inter.node, sid) {
                continue;
            }
            let (a, b) = roads.segment_points(sid);
            let away = (if seg.a == inter.node { b - a } else { a - b }).normalize_or_zero();
            let side = Vec2::new(-away.y, away.x);
            let mid = node.pos + away * nd_sim::walk::crosswalk_setback(roads, inter.node, sid);
            let hw = seg.half_width();
            for sgn in [-1.0f32, 1.0] {
                // Just outside the zebra, on the kerb, facing the far kerb.
                let pole = behind_kerb(mid + side * sgn * (hw + 0.35) + away * (nd_sim::walk::CROSSWALK_HALF + 0.35), corners.by_node.get(&inter.node.0), 0.35);
                let pole = junction::place_pole(node.pos, &mouth_list, sid, pole, 0.35);
                let facing = -side * sgn;
                let lamp = Vec3::new(pole.x + facing.x * 0.3, PED_HEAD_Y, pole.y + facing.y * 0.3);
                out.push(PedSignalPost { inter: k, approach: ap, crosswalk: mid, pole, lamp, heading: facing.y.atan2(facing.x) });
            }
        }
    }
    out
}

/// Lamp positions for the renderer, one per lane feeding a signal, in lane
/// order.
pub fn signal_posts(roads: &RoadNetwork, net: &LaneNetwork) -> Vec<SignalPost> {
    let feeders = feeders(net);
    let mut seen = HashSet::new();
    let mut lanes: Vec<LaneId> = Vec::new();
    for inter in &net.intersections {
        if !inter.signalized {
            continue;
        }
        for &(conn, _) in &inter.connectors {
            for &lid in feeders.get(&conn).map(|v| v.as_slice()).unwrap_or(&[]) {
                if seen.insert(lid) {
                    lanes.push(lid);
                }
            }
        }
    }
    lanes.sort_unstable();
    lanes
        .into_iter()
        .map(|lid| {
            let lane = &net.lanes[lid as usize];
            let back = match lane.kind {
                LaneKind::Road { segment, .. } => junction::extra_stop_back(roads, net, lane.end_node, segment),
                _ => 0.0,
            };
            let (p, d) = lane.point_at((lane.length - 0.6 - back).max(0.0));
            SignalPost { lane: lid, lamp: Vec3::new(p.x - d.x * 0.24, 4.95, p.y - d.y * 0.24), heading: d.y.atan2(d.x) }
        })
        .collect()
}
