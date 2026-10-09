//! Kerbs, junction corners and curb cuts.

use super::*;

// ── kerbs, corners and curb cuts ────────────────────────────────────────────

/// Kerb width; its road-side face is at the carriageway's half width.
pub(super) const KERB_W: f32 = 0.3;
/// Top of a dropped kerb at a crossing: just proud of the asphalt.
pub(super) const KERB_LOW: f32 = ASPHALT_Y + 0.025;
pub(super) const KERB_DROP: f32 = KERB_H - KERB_LOW;
/// Along the kerb, the sloped flare either side of a dropped kerb.
pub(super) const CUT_FLARE: f32 = 0.8;
/// Behind a dropped kerb: the low landing the tactile pad lies on, then the
/// ramp back up to the pavement.
pub(super) const CUT_LANDING: f32 = 0.9;
pub(super) const CUT_RAMP: f32 = 0.8;
/// Depth of a tactile pad (the real ones are 0.8 m deep at a crossing).
pub(super) const TACTILE_DEPTH: f32 = 0.8;
/// How deep a corner's kerb ribbon reaches into the pavement: to the top of
/// a curb cut's ramp. The flat paving fill of the corner begins there.
pub(super) const CORNER_RIBBON: f32 = KERB_W + CUT_LANDING + CUT_RAMP;
/// The paving that fills a junction corner behind its kerb ribbon: a layer
/// over the arms' pavements (KERB_H - 0.01) where it overlaps them.
pub(super) const CORNER_FILL_Y: f32 = KERB_H - 0.007;

/// Where a crosswalk meets the kerbs of the street it crosses: a band
/// `half` either side of `mid` along the street, as far out to either side
/// as the street's pavements reach.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cut {
    /// On the street's centre line, in the middle of the crosswalk.
    pub mid: Vec2,
    /// Along the street.
    pub along: Vec2,
    pub half: f32,
    /// How far across the street (from its centre line) the cut applies.
    pub reach: f32,
}

impl Cut {
    pub(super) fn u(&self, p: Vec2) -> f32 {
        (p - self.mid).dot(self.along)
    }

    pub(super) fn near(&self, p: Vec2) -> bool {
        (p - self.mid).perp_dot(self.along).abs() <= self.reach
    }

    /// How far the kerb at `p` is dropped: 1 across the crosswalk, falling
    /// to 0 over the flares.
    pub(super) fn drop(&self, p: Vec2) -> f32 {
        if !self.near(p) {
            return 0.0;
        }
        let u = self.u(p).abs();
        if u <= self.half {
            1.0
        } else {
            (1.0 - (u - self.half) / CUT_FLARE).max(0.0)
        }
    }

    pub(super) fn on_pad(&self, p: Vec2) -> bool {
        self.near(p) && self.u(p).abs() <= self.half + 1e-3
    }
}

/// The segment joining two nodes, if one is alive.
pub(super) fn segment_between(roads: &RoadNetwork, na: NodeId, nb: NodeId) -> Option<SegmentId> {
    roads.node(na).segments.iter().copied().find(|&s| {
        let sg = roads.segment(s);
        sg.alive && ((sg.a == na && sg.b == nb) || (sg.a == nb && sg.b == na))
    })
}

/// Every place a marked crosswalk meets the kerbs, by the segment crossed:
/// a junction's crosswalk across each arm (where `nd_sim::walk` puts it and
/// the paint goes), mid-block zebras, and the zebra where a footpath
/// crosses a street. The kerbs drop wherever these meet them.
pub(crate) fn crossing_cuts(roads: &RoadNetwork) -> HashMap<u32, Vec<Cut>> {
    let mut out: HashMap<u32, Vec<Cut>> = HashMap::new();
    let mut add = |sid: SegmentId, mid: Vec2, along: Vec2, half: f32| {
        let reach = roads.segment(sid).half_width() + SIDEWALK + 0.5;
        out.entry(sid.0).or_default().push(Cut { mid, along, half, reach });
    };
    for (i, node) in roads.nodes.iter().enumerate() {
        let nid = NodeId(i as u32);
        let streets: Vec<SegmentId> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
        if streets.is_empty() || node.segments.iter().any(|&s| roads.segment(s).alive && roads.segment(s).is_rail()) {
            continue;
        }
        if streets.len() == 2 {
            // A footpath over a street: the zebra across it at the node.
            if node.segments.iter().any(|&s| roads.segment(s).alive && roads.segment(s).kind == RoadKind::Path) {
                let (a, b) = roads.segment_points(streets[0]);
                let d = (b - a).normalize_or_zero();
                for &s in &streets {
                    add(s, node.pos, d, 1.3);
                }
            }
            continue;
        }
        if streets.len() < 3 {
            continue;
        }
        // Nobody crosses a ring's own lanes on a marked crosswalk, nor a
        // highway or a tunnel (`nd_sim::walk::WalkGraph::junction`).
        let on_ring = roads.ring_of_node(nid).is_some();
        for &sid in &streets {
            let seg = roads.segment(sid);
            if seg.fronts_nothing() || (on_ring && roads.ring_of_segment(sid).is_some()) {
                continue;
            }
            let (a, b) = roads.segment_points(sid);
            let away = (if seg.a == nid { b - a } else { a - b }).normalize_or_zero();
            add(sid, node.pos + away * nd_sim::walk::crosswalk_setback(roads, nid, sid), away, nd_sim::walk::CROSSWALK_HALF);
        }
    }
    for (sid, t) in nd_sim::walk::midblock_crossings(roads) {
        let (a, b) = roads.segment_points(sid);
        add(sid, a.lerp(b, t), (b - a).normalize_or_zero(), nd_sim::walk::CROSSWALK_HALF);
    }
    out
}

/// The colours a kerb ribbon is drawn in.
pub(super) struct KerbColours {
    pub(super) kerb: [f32; 4],
    pub(super) paving: [f32; 4],
    pub(super) tactile: [f32; 4],
}

/// A kerb and the pavement behind it along `face` (the kerb's road-side
/// edge), with the pavement on the `out` side (+1 right of travel, as
/// `offset_path` counts it) and `width` deep from the face. `back` closes
/// the pavement's far edge and `caps` its two ends. `normals`, if given,
/// are the exact directions into the pavement at each point of `face` (a
/// rounded corner's: mitred offsets fold over on its short chords).
///
/// Wherever a crosswalk in `cuts` meets it, the kerb drops to just above
/// the road across the crosswalk's width with a sloped flare either side;
/// behind it the pavement dips to a landing and ramps back up, and a
/// tactile pad lies on the landing. Heights are linear in position along
/// each straight piece of the kerb, so a point wherever a cut's band or
/// flare begins or ends is all the shape needs: a plain stretch is four
/// quads a piece, a curb cut a few dozen triangles more.
#[allow(clippy::too_many_arguments)]
pub(super) fn kerb_ribbon(g: &mut Geometry, face: &[Vec2], normals: Option<&[Vec2]>, out: f32, width: f32, back: bool, caps: bool, cuts: &[Cut], c: &KerbColours) {
    if face.len() < 2 || normals.is_some_and(|n| n.len() != face.len()) {
        return;
    }
    let mut pts: Vec<Vec2> = vec![face[0]];
    let mut nrm: Vec<Vec2> = normals.map(|n| vec![n[0]]).unwrap_or_default();
    for (wi, w) in face.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        let (na, nb) = normals.map_or((Vec2::ZERO, Vec2::ZERO), |n| (n[wi], n[wi + 1]));
        let mut ts: Vec<f32> = Vec::new();
        for k in cuts {
            if !(k.near(a) || k.near(b)) {
                continue;
            }
            let (ua, ub) = (k.u(a), k.u(b));
            if (ub - ua).abs() < 1e-5 {
                continue;
            }
            for thr in [-k.half - CUT_FLARE, -k.half, k.half, k.half + CUT_FLARE] {
                let t = (thr - ua) / (ub - ua);
                if t > 1e-3 && t < 1.0 - 1e-3 {
                    ts.push(t);
                }
            }
        }
        ts.sort_by(f32::total_cmp);
        let mut last = 0.0;
        for t in ts {
            if t - last > 1e-3 {
                pts.push(a.lerp(b, t));
                if normals.is_some() {
                    nrm.push(na.lerp(nb, t).normalize_or_zero());
                }
                last = t;
            }
        }
        if b.distance(*pts.last().unwrap()) > 1e-3 {
            pts.push(b);
            if normals.is_some() {
                nrm.push(nb);
            }
        }
    }
    let n = pts.len();
    if n < 2 {
        return;
    }
    let f: Vec<f32> = pts.iter().map(|&p| cuts.iter().map(|k| k.drop(p)).fold(0.0, f32::max)).collect();
    let pad: Vec<Option<usize>> = pts.iter().map(|&p| cuts.iter().position(|k| k.on_pad(p))).collect();
    // Stations across: kerb face, kerb back, end of the landing, top of the
    // ramp, and the pavement's far edge (when it reaches past the ramp).
    let mut st = vec![0.0, KERB_W, KERB_W + CUT_LANDING, CORNER_RIBBON];
    if width > CORNER_RIBBON + 0.01 {
        st.push(width);
    }
    let off = |o: f32| if normals.is_some() { pts.iter().zip(&nrm).map(|(p, n)| *p + *n * o).collect() } else { offset_path(&pts, out * o) };
    let lines: Vec<Vec<Vec2>> = st.iter().map(|&o| off(o)).collect();
    let far = lines.len() - 1;
    let pad_lines = [off(KERB_W + 0.03), off(KERB_W + 0.03 + TACTILE_DEPTH)];
    let top = KERB_H - 0.01;
    let kerb_y = |i: usize| KERB_H - KERB_DROP * f[i];
    let low_y = |i: usize| top - KERB_DROP * f[i];
    let v3 = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
    for i in 0..n - 1 {
        let j = i + 1;
        let d = (pts[j] - pts[i]).normalize_or_zero();
        if d == Vec2::ZERO {
            continue;
        }
        let inward = Vec2::new(-d.y, d.x) * out;
        let inward3 = Vec3::new(inward.x, 0.0, inward.y);
        let (l0, l1) = (&lines[0], &lines[1]);
        g.part(Concrete).quad_facing([v3(l0[i], kerb_y(i)), v3(l0[j], kerb_y(j)), v3(l1[j], kerb_y(j)), v3(l1[i], kerb_y(i))], Vec3::Y, c.kerb);
        g.part(Concrete).quad_facing([v3(l0[i], 0.0), v3(l0[j], 0.0), v3(l0[j], kerb_y(j)), v3(l0[i], kerb_y(i))], -inward3, c.kerb);
        if f[i] == 0.0 && f[j] == 0.0 {
            let lf = &lines[far];
            g.part(Paving).quad_facing([v3(l1[i], top), v3(l1[j], top), v3(lf[j], top), v3(lf[i], top)], Vec3::Y, c.paving);
        } else {
            let (l2, l3) = (&lines[2], &lines[3]);
            g.part(Paving).quad_facing([v3(l1[i], low_y(i)), v3(l1[j], low_y(j)), v3(l2[j], low_y(j)), v3(l2[i], low_y(i))], Vec3::Y, c.paving);
            g.part(Paving).quad_facing([v3(l2[i], low_y(i)), v3(l2[j], low_y(j)), v3(l3[j], top), v3(l3[i], top)], Vec3::Y, c.paving);
            if far > 3 {
                let lf = &lines[far];
                g.part(Paving).quad_facing([v3(l3[i], top), v3(l3[j], top), v3(lf[j], top), v3(lf[i], top)], Vec3::Y, c.paving);
            }
        }
        if pad[i].is_some() && pad[i] == pad[j] {
            let y = top - KERB_DROP + 0.006;
            let (a, b) = (&pad_lines[0], &pad_lines[1]);
            g.part(Tactile).quad_facing([v3(a[i], y), v3(a[j], y), v3(b[j], y), v3(b[i], y)], Vec3::Y, c.tactile);
        }
        if back {
            let lf = &lines[far];
            g.part(Paving).quad_facing([v3(lf[i], 0.0), v3(lf[j], 0.0), v3(lf[j], top), v3(lf[i], top)], inward3, c.paving);
        }
    }
    if caps {
        for (i, k) in [(0, 1), (n - 1, n - 2)] {
            let away = (pts[i] - pts[k]).normalize_or_zero();
            let (a, b) = (lines[0][i], lines[far][i]);
            g.part(Paving).quad_facing([v3(a, 0.0), v3(b, 0.0), v3(b, top), v3(a, kerb_y(i))], Vec3::new(away.x, 0.0, away.y), c.paving);
        }
    }
}

/// One road arriving at a node where its run ends: where its kerbs and
/// pavements begin, and which sides have them.
pub(super) struct ArmEnd {
    pub(super) run: usize,
    /// The run starts at the node (else it ends there).
    pub(super) start: bool,
    /// The segment at the node.
    pub(super) seg: SegmentId,
    /// The run's points from the node outward.
    pub(super) path: Vec<Vec2>,
    pub(super) hw: f32,
    /// Pavement on the counter-clockwise side of the way out (toward the
    /// next arm round), and on the clockwise side.
    pub(super) pave_ccw: bool,
    pub(super) pave_cw: bool,
    /// A one-way link with no pavements of its own (`Segment::ramp`): a
    /// neighbour's kerb rounds the corner onto it and stops.
    pub(super) stub: bool,
    pub(super) ang: f32,
}

/// The kerb round one corner of a junction, from one arm's pavement to the
/// next arm's counter-clockwise.
pub(crate) struct Corner {
    /// The kerb face, with the road on its left: from the first arm's kerb
    /// in toward the junction, round, and out along the second arm's.
    pub kerb: Vec<Vec2>,
    /// At each point of the kerb, the direction into the pavement.
    pub normals: Vec<Vec2>,
    /// Asphalt between a rounded kerb and where the kerb lines would meet.
    pub apron: Vec<Vec2>,
    /// Flat paving behind the kerb's ribbon, out to the arms' back edges.
    pub fill: Vec<Vec2>,
    /// The fill's far edge, closed with a side face.
    pub rim: Vec<Vec2>,
    /// The two arms: (run, starts here, segment).
    pub arms: [(usize, bool, SegmentId); 2],
    /// Signed turn of the kerb at the corner: negative for an ordinary
    /// corner rounded off, positive for one that wraps round the junction.
    pub turn: f32,
    /// One arm has no pavement: the kerb rounds onto it and ends there.
    pub stub: bool,
}

/// Every junction corner, and how far each run's kerb on each side stays
/// back from its end to meet it.
pub(crate) struct Corners {
    pub by_node: BTreeMap<u32, Vec<Corner>>,
    /// (run, at its start, the run's +1 side) → distance from the node
    /// along the run where that kerb begins.
    pub trims: HashMap<(usize, bool, bool), f32>,
}

/// Is a closed polygon free of self-crossings?
pub(super) fn simple_polygon(poly: &[Vec2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let o = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
    let crosses = |a: Vec2, b: Vec2, c: Vec2, d: Vec2| {
        let (d1, d2, d3, d4) = (o(c, d, a), o(c, d, b), o(a, b, c), o(a, b, d));
        d1 * d2 < 0.0 && d3 * d4 < 0.0
    };
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if crosses(poly[i], poly[(i + 1) % n], poly[j], poly[(j + 1) % n]) {
                return false;
            }
        }
    }
    true
}

/// Metres of kerb taper per metre a town street narrows by at a joint.
pub(super) const TOWN_TAPER_PER_M: f32 = 4.0;

/// The longest straight a corner's rounding takes from each kerb
/// (`nd_sim::walk::CORNER_TANGENT_MAX`, which sets the crosswalks back by it).
pub(super) const CORNER_TANGENT_MAX: f32 = nd_sim::walk::CORNER_TANGENT_MAX;

/// The corner between arm `a` and the next arm `b` counter-clockwise at a
/// junction, their kerbs beginning no nearer than `base` to it. The kerb is
/// rounded with a radius of 3 to 6 m by the size of the roads, and each
/// arm's kerb is set back as far as the round needs: the junction's
/// crosswalks then meet the curve itself, as they do at a real corner.
/// Returns the corner and the two set-backs, or `None` where the arms are
/// too close in angle (or too short) to round off.
pub(super) fn corner_between(a: &ArmEnd, b: &ArmEnd, base: f32) -> Option<(Corner, f32, f32)> {
    // A fork at a sharp angle needs its corner far out: the kerb lines of
    // two streets 25 degrees apart meet 20-odd metres from the node. The
    // corner is only refused where it would reach past most of either arm.
    // Half a run each: the corners at its two ends never overlap.
    let (lim_a, lim_b) = ((path_length(&a.path) * 0.5).min(base + 60.0), (path_length(&b.path) * 0.5).min(base + 60.0));
    if base > lim_a || base > lim_b {
        return None;
    }
    let narrow = a.hw.min(b.hw);
    let mut r = (0.6 * narrow).clamp(3.0, 6.0);
    for _ in 0..4 {
        let (mut ea, mut eb) = (base, base);
        for step in 0..5 {
            let (pa, ta) = point_along(&a.path, ea);
            let (pb, tb) = point_along(&b.path, eb);
            let (na, nb) = (Vec2::new(-ta.y, ta.x), Vec2::new(tb.y, -tb.x));
            let (ka, kb) = (pa + na * a.hw, pb + nb * b.hw);
            let den = ta.perp_dot(tb);
            let mut kerb: Vec<Vec2>;
            let mut normals: Vec<Vec2>;
            let mut apron = Vec::new();
            let turn;
            if den.abs() < 0.05 {
                if ta.dot(tb) > 0.0 {
                    return None; // two arms leaving the same way
                }
                // Straight on (a T's through side): the kerbs simply join.
                // Where the road narrows (a lane dropped at a joint) the
                // kerb comes in over a taper along the narrower arm, the
                // asphalt filled out to it, instead of a step across.
                let dw = a.hw - b.hw;
                if dw.abs() > 0.5 {
                    let l = (dw.abs() * TOWN_TAPER_PER_M).clamp(4.0, 14.0);
                    let need = if dw > 0.0 { (0.0, base + l - eb) } else { (base + l - ea, 0.0) };
                    if (need.0 > 0.02 || need.1 > 0.02) && step < 4 {
                        ea += need.0.max(0.0);
                        eb += need.1.max(0.0);
                        if ea > lim_a || eb > lim_b {
                            break;
                        }
                        continue;
                    }
                    // From the wider arm's kerb back to the node along its
                    // edge, across to the narrower's edge and out to where
                    // its kerb begins.
                    let node = pa - ta * ea;
                    apron = if dw > 0.0 { vec![ka, node + na * a.hw, node + nb * b.hw, kb] } else { vec![kb, node + nb * b.hw, node + na * a.hw, ka] };
                }
                kerb = vec![ka, kb];
                normals = vec![na, nb];
                turn = 0.0;
            } else {
                let w = kb - ka;
                let (sa, sb) = (w.perp_dot(tb) / den, w.perp_dot(ta) / den);
                let x = ka + ta * sa;
                let (u1, u2) = (-ta, tb);
                turn = u1.perp_dot(u2).atan2(u1.dot(u2));
                // A corner wrapping round the junction (the outside of a Y)
                // is rounded no wider than the roads, or it would cut into
                // their carriageways.
                let rr = if turn < 0.0 { r } else { r.min(narrow) };
                // The nose of a sharp fork is rounded tightly: at 25 degrees
                // a 3 m radius runs 13 m along each kerb.
                let rr = rr.min(CORNER_TANGENT_MAX / (turn.abs() * 0.5).tan().max(1e-3));
                let td = rr * (turn.abs() * 0.5).tan();
                let (need_a, need_b) = (sa + td, sb + td);
                if (need_a > 0.02 || need_b > 0.02) && step < 4 {
                    ea += need_a.max(0.0);
                    eb += need_b.max(0.0);
                    if ea > lim_a || eb > lim_b {
                        break;
                    }
                    continue;
                }
                let (ti, tj) = (x + ta * td, x + tb * td);
                kerb = vec![ka, ti];
                normals = vec![na, na];
                let centre = ti + Vec2::new(-u1.y, u1.x) * rr * turn.signum();
                let steps = (turn.abs() / 0.21).ceil().max(1.0) as usize;
                for k in 1..steps {
                    let q = centre + Vec2::from_angle(turn * k as f32 / steps as f32).rotate(ti - centre);
                    kerb.push(q);
                    // Toward the centre round an ordinary corner, away from
                    // it round one wrapping the junction.
                    normals.push((q - centre).normalize_or_zero() * turn.signum());
                }
                kerb.push(tj);
                normals.push(nb);
                if turn < 0.0 && td > 0.05 {
                    apron.push(x);
                    apron.extend(kerb[1..].iter().copied());
                }
                kerb.push(kb);
                normals.push(nb);
            }
            if ea > lim_a || eb > lim_b {
                break;
            }
            // Points closer than 2 cm are one (Ti is often the kerb's start).
            let mut k = 1;
            while k < kerb.len() {
                if kerb[k].distance(kerb[k - 1]) < 0.02 {
                    let at = if k == kerb.len() - 1 { k - 1 } else { k };
                    kerb.remove(at);
                    normals.remove(at);
                } else {
                    k += 1;
                }
            }
            if kerb.len() < 2 {
                return None;
            }
            let (oa, ob) = (pa + na * (a.hw + SIDEWALK), pb + nb * (b.hw + SIDEWALK));
            let back: Vec<Vec2> = kerb.iter().zip(&normals).map(|(p, n)| *p + *n * CORNER_RIBBON).collect();
            let mut rim = vec![ob];
            if den.abs() >= 0.05 {
                let w = ob - oa;
                let (sa, sb) = (w.perp_dot(tb) / den, w.perp_dot(ta) / den);
                // Where the arms' back edges meet. At an acute corner that
                // is a little out past where their own pavements begin (the
                // two pavements run into each other there anyway); much
                // further and the fill stops short at the arms' ends.
                if sa <= 6.0 && sb <= 6.0 {
                    rim.push(oa + ta * sa);
                }
            }
            rim.push(oa);
            // The fill's far edge: the arms' back edges to where they meet;
            // where they cross before that (a sharp corner), just the point
            // they meet at; failing both, straight across.
            let mut tries = vec![rim.clone()];
            if rim.len() == 3 {
                tries.push(vec![rim[1]]);
                tries.push(vec![rim[0], rim[2]]);
            }
            let with = |rim: &[Vec2]| back.iter().chain(rim.iter()).copied().collect::<Vec<_>>();
            let (fill, rim) = tries.into_iter().map(|r| (with(&r), r)).find(|(f, _)| simple_polygon(f)).unwrap_or_default();
            let corner = Corner { kerb, normals, apron, fill, rim, arms: [(a.run, a.start, a.seg), (b.run, b.start, b.seg)], turn, stub: false };
            return Some((corner, ea, eb));
        }
        r *= 0.6;
        if r < 1.0 {
            break;
        }
    }
    None
}

/// The corners of every junction (and of every node where two different
/// streets meet end to end) between roads with pavements: highways, ramps,
/// tunnels and footpaths have none, and a roundabout's ring has one only
/// round its outside.
pub(super) fn junction_corners(roads: &RoadNetwork, net: &LaneNetwork, runs: &[Run]) -> Corners {
    let mut ends: BTreeMap<u32, Vec<ArmEnd>> = BTreeMap::new();
    let pave = |sid: SegmentId| {
        let s = roads.segment(sid);
        s.is_street() && !s.is_motorway() && !s.tunnel
    };
    for (ri, run) in runs.iter().enumerate() {
        let n = run.points.len();
        if run.kind != RoadKind::Street || n < 2 {
            continue;
        }
        for start in [true, false] {
            let (node, path, seg) = if start {
                (run.start_node, run.points.clone(), segment_between(roads, run.nodes[0], run.nodes[1]))
            } else {
                (run.end_node, run.points.iter().rev().copied().collect::<Vec<_>>(), segment_between(roads, run.nodes[n - 1], run.nodes[n - 2]))
            };
            let Some(seg) = seg else { continue };
            let sg = roads.segment(seg);
            let d = (path[1] - path[0]).normalize_or_zero();
            if d == Vec2::ZERO {
                continue;
            }
            let hw = sg.half_width();
            let (mut ccw, mut cw) = (pave(seg), pave(seg));
            if sg.ring {
                // Only the outside of the ring has a pavement; its inside
                // is the mountable apron.
                match roads.ring_of_segment(seg).and_then(|k| roads.rings.get(k)) {
                    Some(ring) => {
                        let probe = path[0] + d * 2.0 + Vec2::new(-d.y, d.x) * hw;
                        ccw = ccw && probe.distance(ring.center) > ring.radius;
                        cw = cw && !ccw;
                    }
                    None => (ccw, cw) = (false, false),
                }
            }
            let stub = sg.ramp && !sg.tunnel && !sg.ring && !sg.is_highway();
            ends.entry(node.0).or_default().push(ArmEnd { run: ri, start, seg, path, hw, pave_ccw: ccw, pave_cw: cw, stub, ang: d.y.atan2(d.x) });
        }
    }
    let mut out = Corners { by_node: BTreeMap::new(), trims: HashMap::new() };
    for (node, mut arms) in ends {
        let n = arms.len();
        if n < 2 {
            continue;
        }
        arms.sort_by(|x, y| x.ang.total_cmp(&y.ang).then(x.run.cmp(&y.run)).then(x.start.cmp(&y.start)));
        let base = net.node_radius.get(node as usize).copied().unwrap_or(4.0) + 0.6;
        for i in 0..n {
            let (a, b) = (&arms[i], &arms[(i + 1) % n]);
            // Two pavements meet round the corner; a pavement next to a
            // one-way link without any rounds onto the link and ends, so
            // the corner is not left bare either way.
            let stub = !(a.pave_ccw && b.pave_cw);
            if stub && !((a.pave_ccw && b.stub) || (a.stub && b.pave_cw)) {
                continue;
            }
            let Some((mut corner, ea, eb)) = corner_between(a, b, base) else { continue };
            corner.stub = stub;
            // The counter-clockwise side of the way out is the run's +1
            // side where the run starts here, its -1 side where it ends.
            out.trims.insert((a.run, a.start, a.start), ea);
            out.trims.insert((b.run, b.start, !b.start), eb);
            out.by_node.entry(node).or_default().push(corner);
        }
    }
    out
}

/// A pole meant to stand on the pavement by a junction, moved back behind
/// a corner's kerb (by `clear`) if the rounded corner left it in the road.
pub(super) fn behind_kerb(p: Vec2, corners: Option<&Vec<Corner>>, clear: f32) -> Vec2 {
    let mut best: Option<(f32, Vec2)> = None;
    for corner in corners.into_iter().flatten() {
        for w in corner.kerb.windows(2) {
            let v = (w[1] - w[0]).normalize_or_zero();
            if v == Vec2::ZERO {
                continue;
            }
            let (_, q) = nd_sim::roads::project_on_segment(p, w[0], w[1]);
            let dist = p.distance(q);
            if dist > 4.0 || best.is_some_and(|(d, _)| d <= dist) {
                continue;
            }
            // The pavement is on the kerb's right.
            let block = Vec2::new(v.y, -v.x);
            best = Some((dist, if (p - q).dot(block) < clear { q + block * clear } else { p }));
        }
    }
    best.map(|(_, q)| q).unwrap_or(p)
}
