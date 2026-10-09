//! Lane arrows, drawn apart from the street mesh.

use super::*;

/// Lane arrows painted on each lane arriving at a junction, a few metres
/// before the stop line: one arrow per lane with a head for every movement
/// it allows (`Lane::arrows`), so a left-only lane reads differently from a
/// straight-and-right one. Separate from the street mesh because arrows
/// follow turning demand and the player's choices without the roads
/// changing; rebuild when `LaneNetwork::arrows_version` moves.
///
/// Drawn at road-marking scale (about 5 m long, 0.4 m shafts, filled
/// heads 1.2 m across) in bright paint: the first version used 0.22 m
/// strokes on the dimmed glow material and could not be read from the
/// top-down camera or in daylight. A lane of 9 m or more gets one; long
/// lanes get a second 25 m further back, as real approaches do, so the
/// arrows are seen before the queue covers the first.
pub fn lane_arrows(roads: &RoadNetwork, net: &LaneNetwork, heights: &[f32], inner: &[Vec<(f32, f32)>], pal: &Palette) -> Geometry {
    let mut g = Geometry::default();
    let c = rgba(mul(pal.lane_marker, 1.25));
    let y = MARK_Y + 0.003;
    let cuts = crossing_cuts(roads);
    let radius = |n: NodeId| net.node_radius.get(n.0 as usize).copied().unwrap_or(4.0);
    // [interchange] Where an arm's lanes end at a node (further out at a
    // sharp fork: `LaneNetwork::stop_back`).
    let lane_end = |n: NodeId, s: SegmentId| radius(n) + net.stop_back.get(&(n, s)).copied().unwrap_or(0.0);
    for ap in &net.approaches {
        // How far the painted stop line stands before the lane's end (the
        // lanes end at the junction's radius; the line is drawn beyond the
        // crosswalk, `junction::mouths`), and how much of the lane's start
        // the junction behind it takes.
        let ahead = if pad_node(roads, ap.node) {
            mouths(roads, net, ap.node).iter().find(|m| m.seg == ap.segment).map_or(0.0, |m| (m.marks - lane_end(ap.node, ap.segment)).max(0.0))
        } else {
            0.0
        };
        let seg = roads.segment(ap.segment);
        let from = if seg.a == ap.node { seg.b } else { seg.a };
        let behind = if pad_node(roads, from) {
            mouths(roads, net, from).iter().find(|m| m.seg == ap.segment).map_or(0.0, |m| (m.marks - lane_end(from, ap.segment)).max(0.0))
        } else {
            0.0
        };
        let seg_cuts = cuts.get(&ap.segment.0).map(|v| v.as_slice()).unwrap_or(&[]);
        let (sa, sb) = roads.segment_points(ap.segment);
        for (&l, &a) in ap.lanes.iter().zip(&ap.arrows) {
            let lane = &net.lanes[l as usize];
            if lane.length < 9.0 || a == 0 {
                continue;
            }
            // Clear of every crosswalk on the way: an arrow on a short
            // block used to land on the zebra of the junction behind.
            let on_zebra = |p: Vec2| seg_cuts.iter().any(|k| k.near(p) && k.u(p).abs() < k.half + 0.8);
            // The front 4 m before the stop line (8 m before the lane's end
            // at an ordinary junction), nearer on a short lane; the arrow is
            // 5.4 m from tail to tip.
            let mut back = (ahead + 4.0).max(8.0).min(lane.length - 6.0).max(0.5);
            let mut n = 0;
            while back + 5.4 <= lane.length - behind && n < 2 {
                let (tip_at, d) = lane.point_at(lane.length - back);
                let (tail, _) = lane.point_at(lane.length - back - 5.4);
                if on_zebra(tip_at) || on_zebra(tail) || on_zebra((tip_at + tail) * 0.5) {
                    back += 2.0;
                    continue;
                }
                let from_v = g.get(Paint).positions.len();
                lane_arrow(g.part(Paint), tip_at, d, a, y, c);
                if !heights.is_empty() {
                    // At the height of its own road (the renderer used to
                    // lift it onto whichever road was nearest each vertex,
                    // a deck overhead included).
                    let len = sa.distance(sb).max(1e-3);
                    for p in g.part(Paint).positions[from_v..].iter_mut() {
                        let t = (Vec2::new(p[0], p[2]) - sa).dot(sb - sa) / (len * len);
                        p[1] += nd_sim::landscape::way_height(roads, heights, inner, ap.segment, t);
                    }
                }
                back += 25.0;
                n += 1;
            }
        }
    }
    g
}

/// One painted lane arrow whose foremost point is near `front`, pointing
/// along `d`, with heads for the movements in `arrows` (LEFT, STRAIGHT,
/// RIGHT bits). Right-hand traffic: left is the counter-clockwise side
/// seen from above, `(-d.y, d.x)` negated in sim coordinates (y south).
pub fn lane_arrow(m: &mut crate::mesh::MeshData, front: Vec2, d: Vec2, arrows: u8, y: f32, c: [f32; 4]) {
    use nd_sim::traffic::{LEFT, RIGHT, STRAIGHT};
    let right = Vec2::new(-d.y, d.x);
    let shaft = 0.4;
    let straight = arrows & STRAIGHT != 0;
    let head = |m: &mut crate::mesh::MeshData, base: Vec2, dir: Vec2, len: f32, half: f32| {
        let side = Vec2::new(-dir.y, dir.x);
        m.ground_polygon(&[base + side * half, base + dir * len, base - side * half], y, c);
    };
    // The shaft: up to the straight head, or ending where the turns leave.
    let start = front - d * 5.4;
    let end = if straight { front - d * 1.15 } else { front - d * 2.3 };
    m.ground_strip(&[start, end], shaft, y, c);
    if straight {
        head(m, front - d * 1.2, d, 1.2, 0.6);
    }
    for (bit, sgn) in [(LEFT, -1.0f32), (RIGHT, 1.0)] {
        if arrows & bit == 0 {
            continue;
        }
        // A branch off the shaft well behind the straight head, sweeping out
        // to the side and ending in a head that points across the lane
        // (and a little ahead): a turn, apart from the straight arrow.
        let side = right * sgn;
        let j = if straight { front - d * 3.6 } else { front - d * 2.5 };
        let elbow = j + d * 0.55 + side * 0.4;
        m.ground_strip(&[j - d * 0.1, elbow], shaft, y, c);
        let dir = (side + d * 0.2).normalize_or_zero();
        head(m, elbow - dir * 0.05, dir, 1.05, 0.5);
    }
}

/// Arrows painted along a lane every 45 m (none within 12 m of its ends),
/// pointing the way traffic runs: `forward` along the path.
pub(crate) fn travel_arrows(m: &mut crate::mesh::MeshData, path: &[Vec2], forward: bool, color: [f32; 4]) {
    let len = path_length(path);
    let mut s = 20.0;
    while s < len - 12.0 {
        let (p, d) = point_along(path, s);
        let d = if forward { d } else { -d };
        let side = Vec2::new(-d.y, d.x);
        let tail = p - d * 2.0;
        let tip = p + d * 2.2;
        m.ground_strip(&[tail, p + d * 0.9], 0.22, MARK_Y + 0.003, color);
        for sgn in [-1.0f32, 1.0] {
            m.ground_strip(&[p + d * 0.6 + side * sgn * 0.75, tip], 0.22, MARK_Y + 0.003, color);
        }
        s += 45.0;
    }
}

pub(super) fn dashes_along(mesh: &mut crate::mesh::MeshData, path: &[Vec2], width: f32, y: f32, dash: f32, gap: f32, color: [f32; 4]) {
    let line = Polyline::new(path);
    let len = path_length(path);
    let mut t = gap * 0.5;
    while t + dash <= len {
        let (a, _) = line.at(t);
        let (b, _) = line.at(t + dash);
        mesh.ground_strip(&[a, b], width, y, color);
        t += dash + gap;
    }
}
