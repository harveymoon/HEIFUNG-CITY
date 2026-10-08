//! Junction mouths and pads: where each arm's own carriageway stops and the
//! junction's surface takes over, and where its lane markings stop.
//!
//! Every run's carriageway used to be drawn right through to the node, with
//! a round pad of the junction's radius laid over the middle. That drew the
//! lane lines of every arm across the junction box (a grid of edge and
//! centre lines), bulged the round pad out between arms with no corner (a
//! dark disc over the bare ground, into a roundabout's island, past a dead
//! end), and left the arms' strips overlapping, coplanar, wherever two arms
//! meet at a sharp angle. Now each arm stops at its *mouth* (the junction's
//! radius, or further out where a neighbour at a sharp angle still overlaps
//! it), its markings stop at its stop line, and the pad is the polygon
//! through the arms' mouths and the corners' kerb lines: nothing outside the
//! roads, nothing over the corners' aprons.

use super::*;

/// One street arriving at a junction drawn with a pad.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Mouth {
    pub seg: SegmentId,
    /// Away from the node.
    pub dir: Vec2,
    pub hw: f32,
    /// Distance from the node where the arm's own carriageway begins.
    pub at: f32,
    /// Distance from the node where its lane markings begin: the stop line
    /// beyond its crosswalk, the give-way line at a roundabout, or the mouth.
    pub marks: f32,
    /// A roundabout's ring: its centre and radius. The arm follows the
    /// circle (`dir` is its tangent at the node).
    pub ring: Option<(Vec2, f32)>,
}

impl Mouth {
    /// The middle of the arm's mouth and its direction there (along the
    /// circle for a ring's arm).
    fn mouth(&self, here: Vec2) -> (Vec2, Vec2) {
        match self.ring {
            Some((c, r)) if r > 1.0 => {
                let radial = (here - c).normalize_or_zero();
                let s = if radial.perp_dot(self.dir) >= 0.0 { 1.0 } else { -1.0 };
                let a = radial.to_angle() + s * self.at / r;
                let rm = Vec2::from_angle(a);
                (c + rm * r, Vec2::new(-rm.y, rm.x) * s)
            }
            _ => (here + self.dir * self.at, self.dir),
        }
    }
}

/// The ramp of a highway's merge or diverge at `n` (three streets, two of
/// them highway carriageways): drawn as a taper and gore, no pad.
pub(crate) fn gore_ramp(roads: &RoadNetwork, n: NodeId) -> Option<SegmentId> {
    let alive: Vec<SegmentId> = roads.node(n).segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
    if alive.len() == 3 && alive.iter().filter(|&&s| roads.segment(s).is_highway()).count() == 2 {
        alive.iter().copied().find(|&s| !roads.segment(s).is_highway())
    } else {
        None
    }
}

/// Is `n` a junction drawn with a pad: three or more streets, not a ramp's
/// gore and not a level crossing?
pub(crate) fn pad_node(roads: &RoadNetwork, n: NodeId) -> bool {
    roads.street_degree(n) >= 3 && gore_ramp(roads, n).is_none() && !roads.node(n).segments.iter().any(|&s| roads.segment(s).alive && roads.segment(s).is_rail())
}

/// Does the arm `sid` at junction `n` get a marked crosswalk (as
/// `crossing_cuts` and `nd_sim::walk` have it)?
pub(crate) fn has_crosswalk(roads: &RoadNetwork, n: NodeId, sid: SegmentId) -> bool {
    let seg = roads.segment(sid);
    !(seg.fronts_nothing() || (roads.ring_of_node(n).is_some() && roads.ring_of_segment(sid).is_some()))
}

/// Distance along an arm with half width `hw_a`, at an angle `theta` from a
/// neighbour of half width `hw_b`, past which its side toward the
/// neighbour is clear of the neighbour's carriageway (where the two facing
/// kerb lines meet). Zero for arms 90 degrees or more apart.
pub(crate) fn separation(hw_a: f32, hw_b: f32, theta: f32) -> f32 {
    if theta >= std::f32::consts::FRAC_PI_2 - 1e-3 {
        return 0.0;
    }
    let s = theta.sin().max(0.02);
    (hw_b + hw_a * theta.cos()) / s
}

/// The arms of a junction with their mouths, in counter-clockwise order.
pub(crate) fn mouths(roads: &RoadNetwork, net: &LaneNetwork, n: NodeId) -> Vec<Mouth> {
    let here = roads.node(n).pos;
    let r = net.node_radius.get(n.0 as usize).copied().unwrap_or(4.0) + 0.3;
    let on_ring = roads.ring_of_node(n).is_some();
    let mut arms: Vec<(SegmentId, Vec2, f32, f32, Option<(Vec2, f32)>)> = roads
        .node(n)
        .segments
        .iter()
        .copied()
        .filter(|&s| roads.segment(s).alive && roads.segment(s).is_street())
        .filter_map(|s| {
            let seg = roads.segment(s);
            let far = roads.node(if seg.a == n { seg.b } else { seg.a }).pos;
            let len = far.distance(here);
            let mut d = (far - here).normalize_or_zero();
            // A ring's piece leaves along the circle's tangent, not its chord.
            let ring = roads.ring_of_segment(s).and_then(|k| roads.rings.get(k)).map(|r| (r.center, r.radius));
            if let Some((c, _)) = ring {
                let radial = (here - c).normalize_or_zero();
                let t = Vec2::new(-radial.y, radial.x);
                d = if t.dot(d) >= 0.0 { t } else { -t };
            }
            (d != Vec2::ZERO).then_some((s, d, seg.half_width(), len, ring))
        })
        .collect();
    arms.sort_by(|x, y| x.1.y.atan2(x.1.x).total_cmp(&y.1.y.atan2(y.1.x)).then(x.0 .0.cmp(&y.0 .0)));
    arms.iter()
        .map(|&(sid, d, hw, len, ring)| {
            // Out past any neighbour at a sharp angle that still overlaps it,
            // but never past most of the piece itself.
            let sep = arms.iter().filter(|o| o.0 != sid).map(|o| separation(hw, o.2, d.dot(o.1).clamp(-1.0, 1.0).acos())).fold(0.0f32, f32::max);
            let at = r.max(sep.min((len - 1.0).max(r)));
            let marks = if on_ring && roads.ring_of_segment(sid).is_none() {
                r + 0.9
            } else if has_crosswalk(roads, n, sid) && !on_ring {
                nd_sim::walk::crosswalk_setback(roads, n, sid) + nd_sim::walk::CROSSWALK_HALF + 0.5
            } else {
                at
            };
            Mouth { seg: sid, dir: d, hw, at, marks: marks.max(at), ring }
        })
        .collect()
}

/// A point of a pad's outline and the arms whose heights it takes: one arm
/// for a point of a mouth, two for a corner where their kerb lines meet.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PadPoint {
    pub p: Vec2,
    pub arms: [usize; 2],
}

/// The pad of a junction at `here` with arms `m` (counter-clockwise): each
/// arm's mouth across its carriageway, and between neighbours the point
/// where their facing kerb lines meet if that lies inside both mouths (the
/// corner's apron lies beyond it), else the straight edge between mouths.
pub(crate) fn pad_outline(here: Vec2, m: &[Mouth]) -> Vec<PadPoint> {
    let mut out = Vec::new();
    let k = m.len();
    for i in 0..k {
        let a = &m[i];
        let (mid, along) = a.mouth(here);
        let left = Vec2::new(-along.y, along.x);
        // The clockwise side of the arm, then its counter-clockwise side.
        out.push(PadPoint { p: mid - left * a.hw, arms: [i, i] });
        out.push(PadPoint { p: mid + left * a.hw, arms: [i, i] });
        if k < 2 {
            continue;
        }
        let j = (i + 1) % k;
        let b = &m[j];
        // Two pieces of a ring: the circle runs on between them.
        if a.ring.is_some() && b.ring.is_some() {
            continue;
        }
        let left = Vec2::new(-a.dir.y, a.dir.x);
        let lb = Vec2::new(-b.dir.y, b.dir.x);
        // Kerb lines: a's counter-clockwise side, b's clockwise side.
        let (pa, pb) = (here + left * a.hw, here - lb * b.hw);
        let den = a.dir.perp_dot(b.dir);
        if den.abs() < 0.05 {
            continue;
        }
        let w = pb - pa;
        let (sa, sb) = (w.perp_dot(b.dir) / den, w.perp_dot(a.dir) / den);
        if sa > 0.05 && sb > 0.05 && sa <= a.at + 0.01 && sb <= b.at + 0.01 {
            out.push(PadPoint { p: pa + a.dir * sa, arms: [i, j] });
        }
    }
    out
}

impl crate::mesh::MeshData {
    /// A fan of triangles from `centre` round the closed `ring`, facing up,
    /// with the ground's world-axis texture (as `ground_polygon`), so a pad
    /// whose points stand at different heights still matches the asphalt
    /// strips' texture.
    pub(crate) fn ground_fan(&mut self, centre: Vec3, ring: &[Vec3], color: [f32; 4]) {
        let n = ring.len();
        if n < 2 {
            return;
        }
        let basis = (Vec3::X, Vec3::NEG_Z);
        let base = self.positions.len() as u32;
        self.positions.push(centre.to_array());
        for p in ring {
            self.positions.push(p.to_array());
        }
        for k in 0..=n {
            let p = if k == 0 { centre } else { ring[k - 1] };
            self.normals.push([0.0, 1.0, 0.0]);
            self.colors.push(color);
            self.uvs.push([p.dot(basis.0), p.dot(basis.1)]);
            self.tangents.push([1.0, 0.0, 0.0, 1.0]);
        }
        for k in 0..n as u32 {
            let (a, b) = (base + 1 + k, base + 1 + (k + 1) % n as u32);
            // Counter-clockwise in (x, z) seen from +Y winds clockwise once y
            // becomes z (as `ground_polygon` has it).
            let (pa, pb) = (ring[k as usize], ring[((k + 1) % n as u32) as usize]);
            let ccw = (pa - centre).x * (pb - centre).z - (pa - centre).z * (pb - centre).x > 0.0;
            if ccw {
                self.indices.extend_from_slice(&[base, b, a]);
            } else {
                self.indices.extend_from_slice(&[base, a, b]);
            }
        }
    }
}

/// How far along arm `seg` of a junction (arms `m`) a kerb on its
/// counter-clockwise (`ccw`) or clockwise side must stay back for the kerb
/// and the pavement behind it to be clear of every neighbour on that side:
/// its carriageway and its pavement. Arms 90 degrees or more apart never
/// overlap. Used where the side has no rounded corner of its own (one could
/// not be fitted): such a kerb used to start at the junction's radius
/// whatever the angle and ran across a neighbour's lanes.
pub(crate) fn kerb_clearance(m: &[Mouth], seg: SegmentId, ccw: bool) -> f32 {
    let Some(a) = m.iter().find(|x| x.seg == seg) else { return 0.0 };
    m.iter()
        .filter(|o| o.seg != seg && (a.dir.perp_dot(o.dir) > 0.0) == ccw)
        .map(|o| {
            let theta = a.dir.dot(o.dir).clamp(-1.0, 1.0).acos();
            let sw = nd_sim::zoning::SIDEWALK;
            separation(a.hw, o.hw + sw, theta).max(separation(a.hw + sw, o.hw, theta)) + 0.3
        })
        .fold(0.0, f32::max)
}

/// The kerbed end of a street that just stops, at `p` on its centre line
/// with `d` pointing out past the end: a kerb across the carriageway with
/// the pavement behind it, and the two squares of paving that join it to
/// the side pavements. A round pad used to bulge out past the end there.
pub(super) fn cul_de_sac(g: &mut Geometry, p: Vec2, d: Vec2, hw: f32, c: &KerbColours) {
    let right = Vec2::new(-d.y, d.x);
    let sw = nd_sim::zoning::SIDEWALK;
    kerb_ribbon(g, &[p + right * hw, p - right * hw], None, 1.0, sw, true, false, &[], c);
    let top = KERB_H - 0.01;
    for side in [-1.0f32, 1.0] {
        let r = right * side;
        let sq = [p + r * hw, p + r * (hw + sw), p + r * (hw + sw) + d * sw, p + r * hw + d * sw];
        g.part(Paving).ground_polygon(&sq, top, c.paving);
        let v3 = |q: Vec2, y: f32| Vec3::new(q.x, y, q.y);
        for (a, b, out) in [(sq[1], sq[2], r), (sq[2], sq[3], d)] {
            g.part(Paving).quad_facing([v3(a, 0.0), v3(b, 0.0), v3(b, top), v3(a, top)], Vec3::new(out.x, 0.0, out.y), c.paving);
        }
    }
}

/// A pole meant for the pavement by junction `here` (arms `m`), moved out
/// sideways (to `clear` past the kerb) from any arm's carriageway it stands
/// in. Signal and walk-light poles were placed from their own arm alone, so
/// at a junction of five arms or a sharp fork they stood in a neighbour's
/// lanes.
pub(crate) fn keep_off_arms(here: Vec2, m: &[Mouth], p: Vec2, clear: f32) -> Vec2 {
    let mut p = p;
    for _ in 0..4 {
        let Some((a, lat)) = m
            .iter()
            .map(|a| (a, (p - here).perp_dot(a.dir)))
            .find(|(a, lat)| (p - here).dot(a.dir) > -0.5 && lat.abs() < a.hw + clear - 1e-3)
        else {
            break;
        };
        let along = (p - here).dot(a.dir);
        let side = if lat >= 0.0 { 1.0 } else { -1.0 };
        // `perp_dot(dir)` is positive on the clockwise side of the arm.
        let cw = Vec2::new(a.dir.y, -a.dir.x);
        p = here + a.dir * along + cw * side * (a.hw + clear);
    }
    p
}

/// How much further back than the lane's end (the junction's radius) the
/// stop line of arm `seg` at junction `n` is painted, past what an ordinary
/// junction has (4 m: the crosswalk between): the signal's pole and heads
/// go back with it, or at a sharp fork they hang inside the junction.
pub(crate) fn extra_stop_back(roads: &RoadNetwork, net: &LaneNetwork, n: NodeId, seg: SegmentId) -> f32 {
    if !pad_node(roads, n) {
        return 0.0;
    }
    let r = net.node_radius.get(n.0 as usize).copied().unwrap_or(4.0);
    mouths(roads, net, n).iter().find(|m| m.seg == seg).map_or(0.0, |m| (m.marks - r - 4.0).max(0.0))
}

/// Does `p` (at about height `z`) stand on the carriageway of a street:
/// within its half width less `margin`, at its height (a deck overhead or a
/// tunnel under it does not count)? `heights` and `inner` as
/// `StreetInput`'s, empty for a flat city. Street furniture is laid out
/// along each run on its own; at a sharp fork, a five-way junction or round
/// a tight bend a lamp or bin landed in a neighbour's lanes.
pub(crate) fn on_carriageway(roads: &RoadNetwork, heights: &[f32], inner: &[Vec<(f32, f32)>], p: Vec2, z: f32, margin: f32) -> bool {
    roads.segments_in_box(p - Vec2::splat(30.0), p + Vec2::splat(30.0)).any(|o| {
        let os = roads.segment(o);
        if !os.alive || !os.is_street() || os.tunnel {
            return false;
        }
        let (a, b) = roads.segment_points(o);
        let (t, q) = nd_sim::roads::project_on_segment(p, a, b);
        q.distance(p) < os.half_width() - margin && (heights.is_empty() || (nd_sim::landscape::way_height(roads, heights, inner, o, t) - z).abs() < 2.5)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rich() -> LandValue {
        let mut lv = LandValue::new(800.0, 50.0);
        for v in &mut lv.values {
            *v = 0.9;
        }
        lv
    }

    fn draw(roads: &RoadNetwork, net: &LaneNetwork) -> Geometry {
        let lv = rich();
        let pal = Palette::default();
        generate_street(&StreetInput { roads, net, land_value: &lv, palette: &pal, seed: 4, heights: &[], inner: &[], layer: StreetLayer::All })
    }

    /// Junctions of the shapes the QA walk found wrong: a crossroads, a T,
    /// five arms, forks at 25 and 11 degrees, a dead end and a roundabout.
    fn awkward() -> (RoadNetwork, LaneNetwork) {
        use nd_sim::plan::{Gesture, GestureId, Plan};
        let mut plan = Plan::default();
        let mut id = 0;
        let mut road = |plan: &mut Plan, pts: &[Vec2]| {
            id += 1;
            plan.gestures.insert(GestureId(id), Gesture::Road { points: pts.to_vec(), lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        };
        road(&mut plan, &[Vec2::new(-700.0, 0.0), Vec2::new(-500.0, 0.0)]);
        road(&mut plan, &[Vec2::new(-600.0, -100.0), Vec2::new(-600.0, 100.0)]);
        road(&mut plan, &[Vec2::new(-400.0, 0.0), Vec2::new(-250.0, 0.0)]);
        road(&mut plan, &[Vec2::new(-320.0, 0.0), Vec2::new(-320.0, 120.0)]);
        let c = Vec2::new(-100.0, 0.0);
        for k in 0..5 {
            road(&mut plan, &[c, c + Vec2::from_angle((90.0f32 + 72.0 * k as f32).to_radians()) * 120.0]);
        }
        road(&mut plan, &[Vec2::new(60.0, 0.0), Vec2::new(300.0, 0.0)]);
        road(&mut plan, &[Vec2::new(140.0, 0.0), Vec2::new(140.0, 0.0) + Vec2::from_angle(25f32.to_radians()) * 150.0]);
        road(&mut plan, &[Vec2::new(380.0, 0.0), Vec2::new(600.0, 0.0)]);
        road(&mut plan, &[Vec2::new(400.0, 0.0), Vec2::new(400.0, 0.0) + Vec2::from_angle(11f32.to_radians()) * 200.0]);
        road(&mut plan, &[Vec2::new(-200.0, 300.0), Vec2::new(200.0, 300.0)]);
        road(&mut plan, &[Vec2::new(0.0, 200.0), Vec2::new(0.0, 400.0)]);
        plan.gestures.insert(GestureId(99), Gesture::Roundabout { center: Vec2::new(0.0, 300.0), radius: 25.0, lanes: 1 });
        let (built, rings) = plan.built_roads();
        let mut roads = RoadNetwork::default();
        roads.rings = rings;
        for (_, pts, f, b, k) in built {
            roads.add_polyline_kind(&pts, f, b, k);
        }
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        (roads, net)
    }

    /// How far `p` lies outside the nearest street's carriageway (negative
    /// inside it).
    fn outside_carriageways(roads: &RoadNetwork, p: Vec2) -> f32 {
        roads
            .alive_segments()
            .filter(|(_, s)| s.is_street())
            .map(|(sid, s)| {
                // A ring's pieces follow its circle.
                if let Some(ring) = roads.ring_of_segment(sid).and_then(|k| roads.rings.get(k)) {
                    return (p.distance(ring.center) - ring.radius).abs() - s.half_width();
                }
                let (a, b) = roads.segment_points(sid);
                nd_sim::roads::project_on_segment(p, a, b).1.distance(p) - s.half_width()
            })
            .fold(f32::INFINITY, f32::min)
    }

    /// No lane line of any arm is drawn inside a junction: they stop at the
    /// stop line (the edge and centre lines of every arm used to cross the
    /// junction box in a grid).
    #[test]
    fn lane_markings_stop_at_the_junctions() {
        let (roads, net) = awkward();
        let g = draw(&roads, &net);
        let paint = g.get(Paint);
        let mut checked = 0;
        for (i, node) in roads.nodes.iter().enumerate() {
            let n = NodeId(i as u32);
            if !pad_node(&roads, n) || roads.ring_of_node(n).is_some() {
                continue;
            }
            checked += 1;
            let r = net.node_radius[i] - 0.3;
            let inside: Vec<_> = paint.positions.iter().filter(|p| (p[1] - MARK_Y).abs() < 1e-4 && Vec2::new(p[0], p[2]).distance(node.pos) < r).collect();
            assert!(inside.is_empty(), "junction at {:?}: {} lane-line vertices inside, e.g. {:?}", node.pos, inside.len(), inside.first());
        }
        assert!(checked >= 5, "{checked} junctions");
    }

    /// The junction pads stay on the roads: every vertex drawn at the pads'
    /// height lies on a carriageway or in a corner's apron, never out past
    /// a dead end, into a roundabout's island or over the ground between
    /// two arms (the round pad bulged out at all three).
    #[test]
    fn junction_pads_stay_on_the_roads() {
        let (roads, net) = awkward();
        let g = draw(&roads, &net);
        let corners = junction_corners(&roads, &net, &build_runs(&roads));
        let in_apron = |p: Vec2| corners.by_node.values().flatten().any(|c| c.apron.len() >= 3 && (nd_sim::zoning::point_in_polygon(p, &c.apron) || nd_sim::geom::distance_to_outline(p, &c.apron) < 0.05));
        let mut seen = 0;
        for p in g.get(Asphalt).positions.iter().filter(|p| (p[1] - (ASPHALT_Y + 0.01)).abs() < 1e-4) {
            let q = Vec2::new(p[0], p[2]);
            seen += 1;
            assert!(outside_carriageways(&roads, q) < 0.1 || in_apron(q), "pad vertex off the roads at {q:?}");
        }
        assert!(seen > 50);
        // The roundabout's island is open land: no asphalt over its middle.
        let ring = roads.rings[0];
        let island = ring.radius - ring.half_width() - 0.5;
        let bad: Vec<_> = g.get(Asphalt).positions.iter().filter(|p| Vec2::new(p[0], p[2]).distance(ring.center) < island).collect();
        assert!(bad.is_empty(), "asphalt in the island: {bad:?} (island radius {island}, centre {:?})", ring.center);
    }

    /// Where two arms meet at a sharp angle their carriageways overlap for
    /// tens of metres: nowhere are two pieces of asphalt drawn at the same
    /// height over the same spot (they flickered through each other).
    #[test]
    fn no_coplanar_asphalt_at_forks() {
        let (roads, net) = awkward();
        let g = draw(&roads, &net);
        let m = g.get(Asphalt);
        let tris: Vec<([Vec2; 3], f32)> = m
            .indices
            .chunks(3)
            .filter_map(|t| {
                let p = [t[0], t[1], t[2]].map(|i| m.positions[i as usize]);
                let flat = (p[0][1] - p[1][1]).abs() < 1e-4 && (p[1][1] - p[2][1]).abs() < 1e-4;
                flat.then(|| (p.map(|v| Vec2::new(v[0], v[2])), p[0][1]))
            })
            .collect();
        let inside = |t: &[Vec2; 3], q: Vec2| {
            let s = |a: Vec2, b: Vec2| (b - a).perp_dot(q - a);
            let (d0, d1, d2) = (s(t[0], t[1]), s(t[1], t[2]), s(t[2], t[0]));
            (d0 > 1e-3 && d1 > 1e-3 && d2 > 1e-3) || (d0 < -1e-3 && d1 < -1e-3 && d2 < -1e-3)
        };
        for at in [Vec2::new(140.0, 0.0), Vec2::new(400.0, 0.0), Vec2::new(-100.0, 0.0), Vec2::new(-600.0, 0.0)] {
            for gx in -30..=60 {
                for gy in -15..=30 {
                    let q = at + Vec2::new(gx as f32, gy as f32) * 0.7 + Vec2::splat(0.013);
                    let ys: Vec<f32> = tris.iter().filter(|(t, _)| inside(t, q)).map(|(_, y)| *y).collect();
                    for (i, a) in ys.iter().enumerate() {
                        assert!(!ys[i + 1..].iter().any(|b| (a - b).abs() < 5e-4), "two asphalt surfaces at {a} over {q:?}");
                    }
                }
            }
        }
    }

    /// Nothing that stands up (lamps, poles, bins, trees, signal and walk
    /// light poles) stands on any street's lanes; above head height the
    /// signal arms and heads hang over them on purpose.
    #[test]
    fn no_furniture_on_the_lanes() {
        let (roads, net) = awkward();
        let g = draw(&roads, &net);
        for s in [Matte, Metal, Wood, Glass, Panel, Foliage] {
            for p in &g.get(s).positions {
                if p[1] < 0.3 || p[1] > 3.5 {
                    continue;
                }
                let q = Vec2::new(p[0], p[2]);
                let on = roads.alive_segments().filter(|(_, sg)| sg.is_street()).any(|(sid, sg)| {
                    let (a, b) = roads.segment_points(sid);
                    nd_sim::roads::project_on_segment(q, a, b).1.distance(q) < sg.lanes() as f32 * LANE_WIDTH * 0.5 - 0.2
                });
                assert!(!on, "{s:?} at {:?} stands on the lanes", p);
            }
        }
    }

    /// A street that just ends is kerbed round its end: a kerb across it
    /// and no asphalt out past it.
    #[test]
    fn a_dead_end_is_kerbed_round() {
        let (roads, net) = awkward();
        let g = draw(&roads, &net);
        // The crossroads' west arm ends at x = -700.
        let past = g.get(Asphalt).positions.iter().any(|p| p[0] < -700.05 && p[2].abs() < 8.0);
        assert!(!past, "asphalt past the end");
        let near: Vec<_> = g.get(Concrete).positions.iter().filter(|p| (p[0] + 700.0).abs() < 1.0 && p[2].abs() < 6.0).collect();
        let kerb = near.iter().filter(|p| (p[0] + 700.0).abs() < 0.4 && p[2].abs() < 5.01 && (p[1] - KERB_H).abs() < 1e-3).count();
        assert!(kerb >= 2, "a kerb across the end: {near:?}");
    }

    /// Lane arrows on a short block clear the zebra of the junction behind
    /// them, and at a fork they stand before its stop line, not inside it.
    #[test]
    fn lane_arrows_keep_off_the_crosswalks() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        for x in [-15.0f32, 15.0] {
            roads.add_polyline(&[Vec2::new(x, -100.0), Vec2::new(x, 100.0)], 1, 1);
        }
        let fork = Vec2::new(0.0, 300.0);
        roads.add_polyline(&[Vec2::new(-150.0, 300.0), Vec2::new(150.0, 300.0)], 1, 1);
        roads.add_polyline(&[fork, fork + Vec2::from_angle(25f32.to_radians()) * 150.0], 1, 1);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let g = lane_arrows(&roads, &net, &[], &[], &Palette::default());
        let all: Vec<Cut> = crossing_cuts(&roads).values().flatten().copied().collect();
        let m = g.get(Paint);
        assert!(m.positions.len() > 20, "arrows are drawn");
        for p in &m.positions {
            let q = Vec2::new(p[0], p[2]);
            assert!(!all.iter().any(|k| k.near(q) && k.u(q).abs() < k.half), "arrow on a zebra at {q:?}");
        }
        let n = roads.node_near(fork, 1.0).unwrap();
        let mo = mouths(&roads, &net, n);
        for p in &m.positions {
            let q = Vec2::new(p[0], p[2]) - fork;
            for a in &mo {
                let along = q.dot(a.dir);
                assert!(!(q.perp_dot(a.dir).abs() < a.hw && along > 0.0 && along < a.marks - 0.1), "arrow inside the fork at {q:?} (stop line at {})", a.marks);
            }
        }
    }

    /// Where track crosses a street the rails lie flush in the road: no
    /// ballast, sleeper or rail stands over the carriageway or the
    /// pavements; and a path crossing a street stops at its pavements.
    #[test]
    fn a_level_crossing_lies_flush() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
        roads.add_polyline_kind(&[Vec2::new(-100.0, -20.0), Vec2::new(100.0, 20.0)], 0, 0, RoadKind::Rail);
        roads.add_polyline_kind(&[Vec2::new(-80.0, 50.0), Vec2::new(80.0, 50.0)], 1, 1, RoadKind::Path);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let g = draw(&roads, &net);
        let (road, hw) = (LANE_WIDTH + SHOULDER, LANE_WIDTH + SHOULDER + SIDEWALK);
        for s in [Dirt, Wood, Metal] {
            for p in &g.get(s).positions {
                // Nothing over the carriageway but the flush panel and
                // rails; no ballast or sleepers on the pavements either
                // (trees and benches stand on them).
                if p[0].abs() < road - 0.05 && p[2].abs() < 6.0 {
                    assert!(p[1] < MARK_Y + 0.02, "{s:?} at {p:?} stands over the carriageway");
                }
                if s == Dirt && p[0].abs() < hw - 0.05 && p[2].abs() < 6.0 {
                    panic!("ballast at {p:?} on the pavement");
                }
            }
        }
        assert!(g.get(Dirt).positions.iter().any(|p| p[0] > hw + 1.0), "the track runs on beyond");
        assert!(!g.get(Paving).positions.iter().any(|p| p[0].abs() < LANE_WIDTH + SHOULDER - 0.05 && (p[2] - 50.0).abs() < 1.5), "path paving over the carriageway");
    }
}
