//! Where crossings sit at a junction (how far back from the corner) and
//! where a long street gets a midblock crossing; also read by traffic and
//! the street meshes.

use super::*;

/// Live streets at a node.

/// Widest half width of the streets at a node.
pub(super) fn widest(roads: &RoadNetwork, n: NodeId) -> f32 {
    roads.streets_at(n).map(|s| roads.segment(s).half_width()).fold(0.0, f32::max)
}

/// [interchange] How much further out than the junction's radius
/// (`node_radius`, `r`) the lanes of arm `seg` at `node` end, so cars stop
/// as far back from the painted stop line as at any junction: at a sharp
/// fork the crosswalk and stop line stand out past the rounded kerb
/// (`acute_setback`), and cars used to drive on to the junction's radius,
/// metres past the line. Zero for an ordinary arm, a roundabout, a ramp's
/// gore, the highway and its slip roads.
pub fn stop_back(roads: &RoadNetwork, node: NodeId, seg: SegmentId, r: f32) -> f32 {
    let s = roads.segment(seg);
    if roads.street_degree(node) < 3 || s.fronts_nothing() || roads.ring_of_node(node).is_some() {
        return 0.0;
    }
    if roads.node(node).segments.iter().any(|&o| roads.segment(o).alive && (roads.segment(o).is_motorway() || roads.segment(o).is_rail())) {
        return 0.0;
    }
    // The stop line (`nd_procgen::street_junction::mouths`) stands half a
    // metre past the crosswalk; an ordinary junction's lanes end 4 m in
    // from it.
    (crosswalk_setback(roads, node, seg) + CROSSWALK_HALF + 0.5 - r - 4.0).max(0.0)
}

/// A junction of three or more streets: its arms get crosswalks.
pub fn is_junction(roads: &RoadNetwork, n: NodeId) -> bool {
    roads.street_degree(n) >= 3
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
pub(super) fn acute_setback(roads: &RoadNetwork, node: NodeId, seg: SegmentId) -> f32 {
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
/// [r4-flags-look] (flag day 3 at (974,417)) Nor out on a raised deck or a
/// bridge: a zebra painted over the highway on an overpass's span led
/// nowhere (nothing fronts a deck); the run's zebra that would fall there
/// is left out.
pub fn midblock_crossings(roads: &RoadNetwork) -> Vec<(SegmentId, f32)> {
    let plain = |n: NodeId| roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive).count() == 2 && roads.street_degree(n) == 2;
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
                    let sg = roads.segment(s);
                    if !(sg.viaduct || sg.bridge) {
                        let u = ((target - acc) / len.max(1e-3)).clamp(0.05, 0.95);
                        out.push((s, if fwd { u } else { 1.0 - u }));
                    }
                    break;
                }
                acc += len;
            }
        }
    }
    out
}
