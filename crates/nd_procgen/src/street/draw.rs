//! The street mesh: [`generate_street`] and its passes (runs, nodes,
//! corners, mid-block crossings, signals).

use super::*;

/// Everything the passes of [`generate_street`] share, worked out once:
/// the runs and their height profiles, the junction corners, the tapers,
/// the curb cuts, each junction's mouths and each ramp's gore.
pub(super) struct StreetCtx<'a> {
    pub(super) inp: &'a StreetInput<'a>,
    pub(super) pal: &'a Palette,
    pub(super) roads: &'a RoadNetwork,
    pub(super) net: &'a LaneNetwork,
    /// Node heights (`StreetInput::heights`); empty on a flat city.
    pub(super) heights: &'a [f32],
    pub(super) asphalt_c: [f32; 4],
    pub(super) edge_c: [f32; 4],
    pub(super) centre_c: [f32; 4],
    pub(super) divider_c: [f32; 4],
    pub(super) paving_c: [f32; 4],
    pub(super) kerb_c: [f32; 4],
    pub(super) kerb_cols: KerbColours,
    pub(super) runs: &'a [Run],
    /// Each run's height profile (none on a flat city), kept for the
    /// junction corners, which are set on the profiles of the two runs
    /// they join.
    pub(super) profiles: &'a [(Vec<Vec2>, Vec<f32>)],
    /// The profiles binned for lifting (`None` for a run on the datum).
    pub(super) lifts: Vec<Option<RunProfile<'a>>>,
    pub(super) corners: Corners,
    /// Whether each run is drawn worn (as the run pass decides it).
    pub(super) run_worn: Vec<bool>,
    /// The run arriving at each node on each segment.
    pub(super) arm_run: HashMap<(u32, u32), usize>,
    /// Where a road carries on with fewer or more lanes: (node, narrower
    /// run, taper path from the node, half widths at its two ends).
    pub(super) tapers: Vec<(NodeId, usize, Vec<Vec2>, f32, f32)>,
    pub(super) cuts: HashMap<u32, Vec<Cut>>,
    /// Each junction's arms with their mouths (`junction::mouths`).
    pub(super) pad_mouths: HashMap<u32, Vec<Mouth>>,
    /// Each ramp's join with a highway: the ramp and where its own strips
    /// and markings begin.
    pub(super) gores: HashMap<u32, (SegmentId, f32)>,
    /// Plain joints: nodes where two runs of street meet end to end
    /// without carrying on as one (the lanes change, or a one-way street
    /// turns round), and the two run ends there (run, at its start).
    pub(super) joints: HashMap<u32, [(usize, bool); 2]>,
}

impl<'a> StreetCtx<'a> {
    pub(super) fn new(inp: &'a StreetInput<'a>, runs: &'a [Run], profiles: &'a [(Vec<Vec2>, Vec<f32>)]) -> Self {
        let (pal, roads, net) = (inp.palette, inp.roads, inp.net);
        // Everything a street draws stands within its pavements and wires
        // (or a ramp's gore): well inside this reach.
        let lifts: Vec<Option<RunProfile>> = profiles.iter().map(|(p, z)| z.iter().any(|h| *h != 0.0).then(|| RunProfile::new(p, z, 40.0))).collect();
        let corners = junction_corners(roads, net, runs);
        let run_worn: Vec<bool> = runs.iter().map(|r| inp.land_value.at(point_along(&r.points, path_length(&r.points) * 0.5).0) < 0.35).collect();
        let mut arm_run: HashMap<(u32, u32), usize> = HashMap::new();
        for (ri, r) in runs.iter().enumerate() {
            if let Some(sg) = r.end_segs[0] {
                arm_run.insert((r.start_node.0, sg.0), ri);
            }
            if let Some(sg) = r.end_segs[1] {
                arm_run.insert((r.end_node.0, sg.0), ri);
            }
        }
        let tapers: Vec<(NodeId, usize, Vec<Vec2>, f32, f32)> = (0..roads.nodes.len())
            .filter_map(|i| {
                let n = NodeId(i as u32);
                let alive: Vec<SegmentId> = roads.node(n).segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
                if alive.len() != 2 || roads.node(n).segments.iter().any(|&s| roads.segment(s).alive && !roads.segment(s).is_street()) {
                    return None;
                }
                lane_taper_at(roads, runs, n, &alive).map(|(ri, path, h0, h1)| (n, ri, path, h0, h1))
            })
            .collect();
        let cuts = crossing_cuts(roads);
        let pad_mouths: HashMap<u32, Vec<Mouth>> = (0..roads.nodes.len() as u32).filter(|&i| pad_node(roads, NodeId(i))).map(|i| (i, mouths(roads, net, NodeId(i)))).collect();
        let gores: HashMap<u32, (SegmentId, f32)> = (0..roads.nodes.len() as u32)
            .filter_map(|i| {
                let ramp = gore_ramp(roads, NodeId(i))?;
                let gl = gore_lines(roads, NodeId(i), ramp)?;
                Some((i, (ramp, if gl.clear.is_finite() { gl.clear } else { gl.parted })))
            })
            .collect();
        let mut joints: HashMap<u32, [(usize, bool); 2]> = HashMap::new();
        for i in 0..roads.nodes.len() as u32 {
            let n = NodeId(i);
            if pad_mouths.contains_key(&i) || gores.contains_key(&i) || tapers.iter().any(|t| t.0 == n) {
                continue;
            }
            let alive: Vec<SegmentId> = roads.node(n).segments.iter().copied().filter(|&s| roads.segment(s).alive).collect();
            if alive.len() != 2 || alive.iter().any(|&s| !roads.segment(s).is_street() || roads.segment(s).ring || roads.segment(s).tunnel) {
                continue;
            }
            let ends: Vec<(usize, bool)> = alive.iter().filter_map(|&s| arm_run.get(&(i, s.0)).map(|&ri| (ri, runs[ri].start_node == n && runs[ri].end_segs[0] == Some(s)))).collect();
            if ends.len() == 2 && ends[0].0 != ends[1].0 {
                joints.insert(i, [ends[0], ends[1]]);
            }
        }
        let (paving_c, kerb_c) = (rgba(pal.paving), rgba(pal.curb));
        Self {
            inp,
            pal,
            roads,
            net,
            heights: inp.heights,
            asphalt_c: rgba(pal.asphalt),
            edge_c: rgba(pal.lane_marker),
            centre_c: rgba(pal.centre_line),
            divider_c: rgba(pal.lane_divider),
            paving_c,
            kerb_c,
            kerb_cols: KerbColours { kerb: kerb_c, paving: paving_c, tactile: rgba(pal.tactile) },
            runs,
            profiles,
            lifts,
            corners,
            run_worn,
            arm_run,
            tapers,
            cuts,
            pad_mouths,
            gores,
            joints,
        }
    }

    /// #9 (vi) At a plain joint where run `ri` ends (at its start or
    /// not) and the road bends, the line through the node its carriageway
    /// is cut along: the bisector of the bend, which the other run's
    /// carriageway is cut along too, so the two meet edge to edge instead
    /// of both running on to the node with square ends and overlapping
    /// in the same plane on the inside of the bend. `None` on a straight
    /// joint (square ends already meet) or a hairpin.
    pub(super) fn joint_cut(&self, ri: usize, start: bool) -> Option<(Vec2, Vec2)> {
        let run = &self.runs[ri];
        let node = if start { run.start_node } else { run.end_node };
        let ends = self.joints.get(&node.0)?;
        let &(oi, ostart) = ends.iter().find(|e| **e != (ri, start))?;
        let other = &self.runs[oi];
        let (n, k) = (run.points.len(), other.points.len());
        if n < 2 || k < 2 {
            return None;
        }
        let here = self.roads.node(node).pos;
        let mine = if start { run.points[1] } else { run.points[n - 2] };
        let away = if ostart { other.points[1] } else { other.points[k - 2] };
        let (din, dout) = ((here - mine).normalize_or_zero(), (away - here).normalize_or_zero());
        let bend = din.dot(dout);
        if !(bend < 0.9999 && bend > 0.5) {
            return None;
        }
        let w = (din.perp() + dout.perp()).normalize_or_zero();
        (w != Vec2::ZERO).then_some((here, w))
    }

    pub(super) fn node_r(&self, n: NodeId) -> f32 {
        self.net.node_radius.get(n.0 as usize).copied().unwrap_or(4.0)
    }

    pub(super) fn node_z(&self, n: NodeId) -> f32 {
        self.heights.get(n.0 as usize).copied().unwrap_or(0.0)
    }

    /// The height of run `ri` beside `p` (0 on the datum).
    pub(super) fn run_z(&self, ri: usize, p: Vec2) -> f32 {
        self.lifts.get(ri).and_then(|l| l.as_ref()).map_or(0.0, |l| l.height(p))
    }

    /// Whether `p` lies on some carriageway at height `z`.
    pub(super) fn on_carriageway(&self, p: Vec2, z: f32) -> bool {
        junction::on_carriageway(self.roads, self.heights, self.inp.inner, p, z, 0.05)
    }

    /// (carriageway, markings) set-back of a run's end at `node` on `seg`:
    /// a junction's mouth and stop line, or where a ramp is clear of the
    /// highway it joins.
    pub(super) fn end_trim(&self, node: NodeId, seg: Option<SegmentId>) -> (f32, f32) {
        let Some(seg) = seg else { return (0.0, 0.0) };
        if let Some(m) = self.pad_mouths.get(&node.0).and_then(|v| v.iter().find(|m| m.seg == seg)) {
            return (m.at, m.marks);
        }
        match self.gores.get(&node.0) {
            Some(&(ramp, d)) if ramp == seg => (d, d),
            _ => (0.0, 0.0),
        }
    }

    /// [sweep-visual] The other arms of a pad junction, as polylines with
    /// their half widths (only those that front nothing, a ramp or a
    /// highway, with `ramps_only`).
    pub(super) fn others_at(&self, node: NodeId, seg: Option<SegmentId>, ramps_only: bool) -> Vec<(Vec<Vec2>, f32)> {
        let Some(list) = self.pad_mouths.get(&node.0) else { return Vec::new() };
        list.iter()
            .filter(|m| Some(m.seg) != seg && (!ramps_only || self.roads.segment(m.seg).fronts_nothing()))
            .filter_map(|m| self.arm_run.get(&(node.0, m.seg.0)).map(|&ri| (self.runs[ri].points.clone(), self.roads.segment(m.seg).half_width())))
            .collect()
    }

    /// [sweep-visual] Whether an arm's overlap with the junction's other
    /// arms runs on past its first piece.
    pub(super) fn overlaps_on(&self, node: NodeId, seg: Option<SegmentId>) -> bool {
        self.pad_mouths.get(&node.0).and_then(|v| seg.and_then(|s| v.iter().find(|m| m.seg == s))).is_some_and(|m| m.want > m.at + 0.5)
    }

    /// A street that just ends (no other street, path or track at its
    /// end): kerbed round its end, no pad.
    pub(super) fn dead_end(&self, n: NodeId) -> bool {
        self.roads.node(n).segments.iter().filter(|&&s| self.roads.segment(s).alive).count() == 1 && self.roads.street_degree(n) == 1
    }

    /// Set the run drawn last on its profile once it is complete.
    pub(super) fn finish_run(&self, g: &mut Geometry, drawn: &mut Option<(Vec<usize>, usize)>) {
        if let Some((from, ri)) = drawn.take() {
            if let Some(Some(profile)) = self.lifts.get(ri) {
                lift_since(g, &from, |p| profile.height(p));
            }
        }
    }
}

/// Move what was drawn since `marks` (surface, vertex and index counts)
/// onto each surface's weathered variant.
pub(super) fn weather_tail(g: &mut Geometry, marks: Vec<(Surface, usize, usize)>) {
    for (s, v, i) in marks {
        if let Some(w) = s.worn() {
            let tail = g.part(s).take_from(v, i);
            g.part(w).append(&tail);
        }
    }
}

/// Where each surface's mesh stands now, for [`weather_tail`].
pub(super) fn worn_marks(g: &Geometry) -> Vec<(Surface, usize, usize)> {
    [Asphalt, Paving, Concrete].iter().map(|s| (*s, g.get(*s).positions.len(), g.get(*s).indices.len())).collect()
}

/// Draw with `draw`, then raise what it drew by `z`.
pub(super) fn at_height(g: &mut Geometry, z: f32, draw: impl FnOnce(&mut Geometry)) {
    let from = marks(g);
    draw(g);
    if z != 0.0 {
        lift_since(g, &from, |_| z);
    }
}

/// A zebra across a carriageway of half width `hw`: stripes from `c0` to
/// `c1` (along the road), a metre apart across it along `right`, but for
/// those `skip` turns down (by their offset across).
#[allow(clippy::too_many_arguments)]
pub(super) fn zebra(g: &mut Geometry, pal: &Palette, c0: Vec2, c1: Vec2, right: Vec2, hw: f32, skip: impl Fn(f32) -> bool) {
    let mut s = -hw + SHOULDER + 0.4;
    while s < hw - SHOULDER - 0.2 {
        if !skip(s) {
            g.part(Paint).ground_strip(&[c0 + right * s, c1 + right * s], 0.5, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 0.9)));
        }
        s += 1.0;
    }
}

pub fn generate_street(inp: &StreetInput) -> Geometry {
    let mut g = Geometry::default();
    let runs = build_runs(inp.roads);
    let node_z = |n: NodeId| inp.heights.get(n.0 as usize).copied().unwrap_or(0.0);
    let profiles: Vec<(Vec<Vec2>, Vec<f32>)> = if inp.heights.is_empty() { Vec::new() } else { runs.iter().map(|r| run_profile(inp.roads, r, &node_z, inp.inner)).collect() };
    let cx = StreetCtx::new(inp, &runs, &profiles);
    draw_runs(&cx, &mut g);
    draw_roundabout_aprons(&cx, &mut g);
    draw_nodes(&cx, &mut g);
    draw_corners(&cx, &mut g);
    draw_midblock(&cx, &mut g);
    draw_ped_signals(&cx, &mut g);
    draw_signals(&cx, &mut g);
    g
}

/// Every run: carriageway, markings, kerbs, pavements and furniture, each
/// set on its profile and (on a cheap street) moved onto the worn surfaces
/// once drawn.
pub(super) fn draw_runs(cx: &StreetCtx, g: &mut Geometry) {
    // Where a neglected run started in each weathered surface's mesh: its
    // carriageway, kerbs and slabs move onto the worn materials once drawn.
    let mut worn_from: Option<Vec<(Surface, usize, usize)>> = None;
    // The run drawn last and where its drawing began, to set it on its
    // profile once it is complete (runs end at several returns).
    let mut drawn: Option<(Vec<usize>, usize)> = None;
    for (ri, run) in cx.runs.iter().enumerate() {
        cx.finish_run(g, &mut drawn);
        if let Some(marks) = worn_from.take() {
            weather_tail(g, marks);
        }
        draw_run(cx, g, ri, run, &mut drawn, &mut worn_from);
    }
    cx.finish_run(g, &mut drawn);
    if let Some(marks) = worn_from.take() {
        weather_tail(g, marks);
    }
}

pub(super) fn draw_run(cx: &StreetCtx, g: &mut Geometry, ri: usize, run: &Run, drawn: &mut Option<(Vec<usize>, usize)>, worn_from: &mut Option<Vec<(Surface, usize, usize)>>) {
    let (pal, roads) = (cx.pal, cx.roads);
    // The run is drawn through its profile's points: a long street's
    // inner height samples become vertices of its carriageway, kerbs
    // and pavements. Drawn through its nodes alone, a street over a
    // rise ran straight from end to end under the profile the ground
    // and the cars follow, and the built ground covered its asphalt.
    let dense = if cx.heights.is_empty() {
        run.points.clone()
    } else {
        *drawn = Some((marks(g), ri));
        cx.profiles[ri].0.clone()
    };
    let seg = roads.segment(run.first);
    if !cx.inp.layer.keeps(seg.tunnel) {
        return;
    }
    let hw = seg.half_width();
    let mut rng = rng_for(cx.inp.seed ^ (run.first.0 as u64 * 7919), 3);
    // A roundabout's ring follows its circle, a vertex every 3 m or so:
    // drawn through its nodes alone it was a polygon, its outer
    // pavement a run of straight facets and its inner edge cutting into
    // the island.
    let on_circle = seg.ring.then(|| roads.ring_of_segment(run.first).and_then(|k| roads.rings.get(k))).flatten().map(|ring| arc_points(&dense, ring.center, ring.radius));
    let pts = on_circle.as_ref().unwrap_or(&dense);
    if path_length(pts) < 0.5 {
        return;
    }
    if run.kind == RoadKind::Path {
        footpath(g, pal, roads, run, pts, &mut rng);
        return;
    }
    if run.kind == RoadKind::Rail {
        railway(g, pal, roads, run, pts);
        return;
    }
    // Cheap streets are neglected: faded markings, potholes, cracked
    // slabs and litter. The land value in the middle of the run decides.
    let lv_mid = cx.inp.land_value.at(point_along(pts, path_length(pts) * 0.5).0);
    let grit = lv_mid < 0.35;
    if grit {
        *worn_from = Some(worn_marks(g));
    }
    // A roundabout's ring has a kerb and pavement round its outside only,
    // which the arms' corners run into; its inner edge is the mountable
    // apron. The side of the run that faces out:
    let ring_out = seg.ring.then(|| roads.ring_of_segment(run.first).and_then(|k| roads.rings.get(k))).flatten().map(|ring| {
        let (mid, d) = point_along(pts, path_length(pts) * 0.5);
        if (mid + Vec2::new(-d.y, d.x) * hw).distance(ring.center) > ring.radius { 1.0f32 } else { -1.0 }
    });
    draw_carriageway(cx, g, ri, run, pts, hw, grit, ring_out);

    // Kerbs and pavements stop short of the junction pads, each side as
    // far back as the rounded corner it runs into needs
    // (`junction_corners`); street furniture keeps to this stretch.
    let t0 = cx.node_r(run.start_node) + 0.6;
    let t1 = cx.node_r(run.end_node) + 0.6;
    let walk = trim(pts, t0, t1);
    if walk.len() < 2 {
        return;
    }
    // Highways and ramps have hard shoulders, not pavements: a low
    // barrier at the edge and lamps, nothing else beside them.
    let motorway = seg.is_motorway() || (seg.ring && ring_out.is_none());
    if motorway {
        draw_hard_shoulders(cx, g, ri, run, &walk, hw);
        return;
    }
    if seg.tunnel {
        // The bore: like a ramp, a way for traffic only (nobody walks
        // or parks in it), so no pavements: a low safety kerb at the
        // shoulder, then the tube (`bore_shell`) with its lamp lines and
        // no street furniture. `nd_procgen::terrain` sizes the portal
        // arch to the tube's walls (`TUNNEL_WALL`).
        for side in [-1.0f32, 1.0] {
            g.part(Concrete).path_box(&walk, side * (hw + 0.2), 0.4, 0.0, 0.35, cx.kerb_c);
        }
        bore_shell(g, &walk, hw, rgba(mul(pal.concrete(0), 0.75)), rgba(mul(pal.concrete(0), 0.5)), rgba(mul(pal.street_lamp, 0.9)));
        return;
    }
    let kerb_ends = draw_kerbs(cx, g, ri, run, pts, &walk, hw, ring_out, (t0, t1));
    if ring_out.is_none() {
        for (start, node) in [(true, run.start_node), (false, run.end_node)] {
            if cx.dead_end(node) {
                let (p, d) = if start { let (p, d) = point_along(pts, 0.0); (p, -d) } else { point_along(pts, path_length(pts)) };
                cul_de_sac(g, p, d, hw, &cx.kerb_cols);
            }
        }
    }
    if let Some(side) = ring_out {
        // Round the ring: lamps on the outer pavement, nothing else.
        let walk_line = Polyline::new(&walk);
        let len = path_length(&walk);
        let mut s = 6.0;
        while s < len - 4.0 {
            let (p, d) = walk_line.at(s);
            let q = p + Vec2::new(-d.y, d.x) * side * (hw + 0.95);
            if !cx.on_carriageway(q, cx.run_z(ri, q)) {
                lamp_post_world(g, q, 7.0, pal.street_lamp);
            }
            s += 28.0;
        }
        return;
    }
    draw_furniture(cx, g, ri, run, pts, &walk, hw, lv_mid, (t0, t1), kerb_ends, &mut rng);
    if grit {
        street_grit(g, pal, &walk, hw, path_length(&walk), &mut rng);
    }
}

/// A run's carriageway, cut back at each end to the junction's mouth (or a
/// ramp's gore), with its edge lines, lane lines, centre line and, on a
/// one-way road, travel arrows.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_carriageway(cx: &StreetCtx, g: &mut Geometry, ri: usize, run: &Run, pts: &Vec<Vec2>, hw: f32, grit: bool, ring_out: Option<f32>) {
    let total = run.lanes_fwd as f32 + run.lanes_bwd as f32;
    let fade = |c: [f32; 4]| if grit { [c[0] * 0.5, c[1] * 0.5, c[2] * 0.5, c[3]] } else { c };
    // The carriageway stops at each junction's mouth, where the pad takes
    // over, and the markings at the stop line (`junction::mouths`); a
    // ramp's at the point it is clear of the highway it joins.
    let [seg0, seg1] = run.end_segs;
    let (c0, m0) = cx.end_trim(run.start_node, seg0);
    let (c1, m1) = cx.end_trim(run.end_node, seg1);
    let cut = |a: f32, b: f32| if a == 0.0 && b == 0.0 { pts.clone() } else { trim(pts, a, b) };
    // [sweep-visual] An arm meeting a neighbour at so sharp an angle
    // that they overlap past its first piece (a ramp drawn into a
    // street): its own strip starts where it is clear of them, its
    // stretch over them drawn a little under their asphalt (no
    // coplanar flicker; it shows only where it runs out beside them)
    // and unmarked.
    let run_len = path_length(pts);
    let rev: Vec<Vec2> = pts.iter().rev().copied().collect();
    let s0 = if cx.overlaps_on(run.start_node, seg0) { clear_along(pts, c0, run_len * 0.45, &[-hw, 0.0, hw], &cx.others_at(run.start_node, seg0, false)) } else { c0 };
    let s1 = if cx.overlaps_on(run.end_node, seg1) { clear_along(&rev, c1, run_len * 0.45, &[-hw, 0.0, hw], &cx.others_at(run.end_node, seg1, false)) } else { c1 };
    let carriage = cut(s0, s1);
    let marked = cut(m0.max(s0), m1.max(s1));
    if carriage.len() >= 2 {
        let cuts = [(s0 == 0.0).then(|| cx.joint_cut(ri, true)).flatten(), (s1 == 0.0).then(|| cx.joint_cut(ri, false)).flatten()];
        if cuts.iter().any(|c| c.is_some()) {
            mitred_strip(g.part(Asphalt), &carriage, hw, ASPHALT_Y, cx.asphalt_c, cuts);
        } else {
            g.part(Asphalt).path_strip(&carriage, 0.0, hw * 2.0, ASPHALT_Y, cx.asphalt_c);
        }
    }
    for under in [(s0 > c0 + 0.1).then(|| trim(pts, c0, (run_len - s0).max(0.0))), (s1 > c1 + 0.1).then(|| trim(pts, (run_len - s1).max(0.0), c1))].into_iter().flatten() {
        if under.len() >= 2 {
            g.part(Asphalt).path_strip(&under, 0.0, hw * 2.0, ASPHALT_Y - 0.03, cx.asphalt_c);
        }
    }
    // Round a ring only the outer edge line is broken at the arms.
    let ring_lines = if ring_out.is_some() { pts.clone() } else { marked.clone() };
    for side in [-1.0f32, 1.0] {
        let line = if ring_out == Some(side) { &marked } else { &ring_lines };
        if line.len() >= 2 {
            g.part(Paint).path_strip(line, side * (hw - SHOULDER * 0.5), 0.18, MARK_Y, fade(cx.edge_c));
        }
    }
    if ring_lines.len() >= 2 {
        for k in 1..(run.lanes_fwd + run.lanes_bwd) as u32 {
            let off = (k as f32 - total * 0.5) * LANE_WIDTH;
            let is_centre = run.lanes_fwd > 0 && run.lanes_bwd > 0 && k == run.lanes_bwd as u32;
            if is_centre {
                g.part(Paint).path_strip(&ring_lines, off + 0.18, 0.14, MARK_Y, fade(cx.centre_c));
                g.part(Paint).path_strip(&ring_lines, off - 0.18, 0.14, MARK_Y, fade(cx.centre_c));
            } else {
                let path = offset_path(&ring_lines, off);
                dashes_along(g.part(Paint), &path, 0.16, MARK_Y, 3.0, 4.0, fade(cx.divider_c));
            }
        }
        // One-way roads (ramps, highways, one-way streets) show which way
        // they run: an arrow in every lane every 45 m.
        if (run.lanes_fwd == 0) != (run.lanes_bwd == 0) {
            let forward = run.lanes_fwd > 0;
            for k in 0..(run.lanes_fwd + run.lanes_bwd) {
                let path = offset_path(&ring_lines, (k as f32 + 0.5 - total * 0.5) * LANE_WIDTH);
                travel_arrows(g.part(Paint), &path, forward, fade(cx.edge_c));
            }
        }
    }
}

/// A carriageway `hw` either side of `pts` (as `path_strip` lays it), its
/// first and last edges cut along the line through `cuts[0]` / `cuts[1]`
/// (a point and a direction) where given.
fn mitred_strip(m: &mut crate::mesh::MeshData, pts: &[Vec2], hw: f32, y: f32, color: [f32; 4], cuts: [Option<(Vec2, Vec2)>; 2]) {
    let mut l = offset_path(pts, -hw);
    let mut r = offset_path(pts, hw);
    let n = l.len();
    for (end, cut) in cuts.into_iter().enumerate() {
        let Some((at, w)) = cut else { continue };
        let (i, j) = if end == 0 { (0, 1) } else { (n - 1, n - 2) };
        for line in [&mut l, &mut r] {
            let (p, e) = (line[i], line[i] - line[j]);
            let den = e.perp_dot(w);
            if den.abs() > 1e-6 {
                line[i] = p + e * ((at - p).perp_dot(w) / den);
            }
        }
    }
    for i in 0..n.saturating_sub(1) {
        m.ground_polygon(&[l[i], l[i + 1], r[i + 1], r[i]], y, color);
    }
}

/// A highway's or ramp's edges: a low barrier each side (broken where it
/// would stand on or right beside another motorway's lanes at its height,
/// and following a taper's edge), and lamps alternating sides.
pub(super) fn draw_hard_shoulders(cx: &StreetCtx, g: &mut Geometry, ri: usize, run: &Run, walk: &[Vec2], hw: f32) {
    let (pal, roads) = (cx.pal, cx.roads);
    let seg = roads.segment(run.first);
    let barrier = rgba(mul(pal.concrete(0), 0.8));
    // Where a ramp runs beside the carriageway to merge or diverge,
    // neither barrier stands between them: a barrier stops where it
    // would be on (or right beside) another motorway's lanes.
    let mine = |sid: SegmentId| {
        let s = roads.segment(sid);
        run.nodes.contains(&s.a) && run.nodes.contains(&s.b)
    };
    // Only a way at this one's height counts: a tunnel (or any way
    // passing well under or over) is not beside it, and the
    // barrier used to stop exactly where a tunnel ramp ran beneath.
    // The run's own height comes from its binned profile (`lifts`),
    // inner samples and all: a list of node heights against the
    // run's points read the first node's height everywhere.
    // Along a taper at either end the barrier follows its edge
    // (`lane_taper`), not the narrower road's.
    let in_taper = |p: Vec2| {
        cx.tapers.iter().filter(|t| t.1 == ri).any(|(_, _, path, h0, _)| {
            let total = path_length(path);
            let mut acc = 0.0;
            path.windows(2).any(|w| {
                let (t, q) = nd_sim::roads::project_on_segment(p, w[0], w[1]);
                let along = acc + w[0].distance(w[1]) * t;
                acc += w[0].distance(w[1]);
                along < total - 0.3 && q.distance(p) < h0 + 1.0
            })
        })
    };
    let blocked = |p: Vec2| {
        in_taper(p) || roads.segments_in_box(p - Vec2::splat(30.0), p + Vec2::splat(30.0)).any(|o| {
            let os = roads.segment(o);
            if mine(o) || !(os.is_motorway() || os.ring) || os.tunnel != seg.tunnel {
                return false;
            }
            let (a, b) = roads.segment_points(o);
            let (t, q) = nd_sim::roads::project_on_segment(p, a, b);
            let oz = nd_sim::landscape::way_height(roads, cx.heights, cx.inp.inner, o, t);
            q.distance(p) < os.half_width() + 1.2 && (oz - cx.run_z(ri, p)).abs() < 2.5
        })
    };
    for side in [-1.0f32, 1.0] {
        let line = offset_path(walk, side * (hw + 0.2));
        let along = Polyline::new(&line);
        let len = path_length(&line);
        let steps = (len / 2.0).ceil().max(1.0) as usize;
        let mut piece: Vec<Vec2> = Vec::new();
        for k in 0..=steps {
            let q = along.at(len * k as f32 / steps as f32).0;
            if blocked(q) {
                if piece.len() >= 2 {
                    g.part(Concrete).path_box(&piece, 0.0, 0.4, 0.0, 0.8, barrier);
                }
                piece.clear();
            } else {
                piece.push(q);
            }
        }
        if piece.len() >= 2 {
            g.part(Concrete).path_box(&piece, 0.0, 0.4, 0.0, 0.8, barrier);
        }
    }
    let walk_line = Polyline::new(walk);
    let len = path_length(walk);
    let mut s = 10.0;
    let mut side = 1.0;
    while s < len - 4.0 {
        let (p, d) = walk_line.at(s);
        let right = Vec2::new(-d.y, d.x);
        // Not on the lanes of a ramp running alongside to merge.
        let q = p + right * side * (hw + 0.5);
        if !cx.on_carriageway(q, cx.run_z(ri, q)) {
            lamp_post_world(g, q, 9.0, pal.street_lamp);
        }
        s += 40.0;
        side = -side;
    }
}

/// A street's kerbs and pavements, each side to its own corners (or, past
/// a ramp joined at a sharp angle, until clear of it), dropped wherever a
/// crosswalk meets them. Returns where each side's kerb begins and ends
/// (set-backs from the run's two ends), -1 side first.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_kerbs(cx: &StreetCtx, g: &mut Geometry, ri: usize, run: &Run, pts: &Vec<Vec2>, walk: &[Vec2], hw: f32, ring_out: Option<f32>, (t0, t1): (f32, f32)) -> [(f32, f32); 2] {
    let roads = cx.roads;
    let run_cuts: Vec<Cut> = run.nodes.windows(2).filter_map(|w| segment_between(roads, w[0], w[1])).flat_map(|s| cx.cuts.get(&s.0).into_iter().flatten().copied()).collect();
    // Where each side's kerb and pavement begin and end (along the run).
    let mut kerb_ends = [(t0, t1); 2];
    for side in [-1.0f32, 1.0] {
        if ring_out.is_some_and(|o| o != side) {
            continue;
        }
        // Without a corner of its own, a kerb stays back until it is
        // clear of the junction's other arms; at a dead end it runs to
        // the end, where `cul_de_sac` closes it.
        let end = |node: NodeId, start: bool, t: f32| {
            if cx.dead_end(node) {
                return (0.0, None);
            }
            // [sweep-visual] Past a ramp or highway joined at a sharp
            // angle (which has no pavement or corner of its own on this
            // side) the kerb and pavement wait until they are clear of
            // its carriageway, however far round its bends that is.
            let seg = run.end_segs[!start as usize];
            let ramps = cx.others_at(node, seg, true);
            let clear = if ramps.is_empty() {
                0.0
            } else {
                let (path, sgn) = if start { (pts.clone(), side) } else { (pts.iter().rev().copied().collect::<Vec<_>>(), -side) };
                let from = cx.node_r(node);
                let c = clear_along(&path, from, path_length(&path) * 0.45, &[sgn * hw, sgn * (hw + SIDEWALK)], &ramps);
                if c > from { c } else { 0.0 }
            };
            let e = match cx.corners.trims.get(&(ri, start, side > 0.0)) {
                Some(&e) => e,
                None => {
                    // The run's +1 side is counter-clockwise of the way
                    // out where it starts here, clockwise where it ends.
                    let ccw = (side > 0.0) == start;
                    t.max(seg.map_or(0.0, |s| junction::kerb_clearance(cx.pad_mouths.get(&node.0).map(|v| v.as_slice()).unwrap_or(&[]), s, ccw)))
                }
            };
            (e.max(clear), (clear > e).then_some(e))
        };
        let ((e0, held0), (e1, held1)) = (end(run.start_node, true, t0), end(run.end_node, false, t1));
        kerb_ends[(side > 0.0) as usize] = (e0, e1);
        // [sweep-visual] Where the kerb waits for a ramp to clear, the
        // stretch the pavement would have taken is surfaced like the
        // junction (under the ramp's own asphalt), not left bare.
        let total = path_length(pts);
        for (start, held, e) in [(true, held0, e0), (false, held1, e1)] {
            let Some(h) = held else { continue };
            let band = if start { trim(pts, h, (total - e).max(0.0)) } else { trim(pts, (total - e).max(0.0), h) };
            if band.len() >= 2 {
                g.part(Asphalt).path_strip(&band, side * (hw + SIDEWALK * 0.5), SIDEWALK, ASPHALT_Y - 0.03, cx.asphalt_c);
            }
        }
        let line = if (e0, e1) == (t0, t1) { walk.to_vec() } else { trim(pts, e0, e1) };
        if line.len() >= 2 {
            kerb_ribbon(g, &offset_path(&line, side * hw), None, side, SIDEWALK, true, true, &run_cuts, &cx.kerb_cols);
        }
    }
    kerb_ends
}

/// Furniture by arc length along a street's pavements, none of it standing
/// in another street's lanes or past its kerbs: lamps, trees on dear
/// streets, wooden poles and wires on cheap ones, bins, benches, kiosks and
/// phone boxes.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_furniture(cx: &StreetCtx, g: &mut Geometry, ri: usize, run: &Run, pts: &[Vec2], walk: &[Vec2], hw: f32, lv_mid: f32, (t0, _t1): (f32, f32), kerb_ends: [(f32, f32); 2], rng: &mut impl Rng) {
    let (pal, roads) = (cx.pal, cx.roads);
    let walk_line = Polyline::new(walk);
    let len = path_length(walk);
    let lamp_off = hw + 0.95;
    let back_off = hw + SIDEWALK - 0.7;
    let at = |s: f32, side: f32, off: f32| {
        let (p, d) = walk_line.at(s);
        let right = Vec2::new(-d.y, d.x);
        (p + right * side * off, d, right)
    };
    let free = |p: Vec2| !cx.on_carriageway(p, cx.run_z(ri, p));
    // On that side's pavement, not out on a junction's asphalt where
    // the kerb starts further along (a fork's wedge).
    let run_len = path_length(pts);
    let kerbed = |s: f32, side: f32| {
        let (e0, e1) = kerb_ends[(side > 0.0) as usize];
        t0 + s >= e0 + 0.8 && t0 + s <= run_len - e1 - 0.8
    };
    let mut lamp_ts: Vec<(f32, f32)> = Vec::new();
    let mut s = 6.0;
    let mut side = 1.0;
    while s < len - 4.0 {
        let (p, _, _) = at(s, side, lamp_off);
        if free(p) && kerbed(s, side) {
            lamp_post_world(g, p, 7.0, pal.street_lamp);
            lamp_ts.push((s, side));
        }
        s += 28.0;
        side = -side;
    }
    if lv_mid > 0.42 {
        let mut s = 4.0;
        while s < len - 3.0 {
            for side in [-1.0f32, 1.0] {
                if lamp_ts.iter().any(|(lt, ls)| *ls == side && (lt - s).abs() < 3.0) {
                    continue;
                }
                let (p, d, _) = at(s, side, back_off);
                if !free(p) || !kerbed(s, side) {
                    continue;
                }
                g.part(Concrete).boxed(Vec3::new(p.x, KERB_H + 0.2, p.y), Vec3::new(1.3, 0.4, 1.3), d.y.atan2(d.x), rgba(mul(pal.paving, 1.1)));
                crate::climate::climate_tree(g, pal, p, rng.random_range(3.0..4.5), rng.random_range(2.8..4.0), mul(pal.foliage, rng.random_range(0.85..1.15)));
            }
            s += 13.0 + rng.random_range(0.0..3.0);
        }
    }
    if lv_mid < 0.4 {
        let mut prev: Option<Vec3> = None;
        let mut s = 2.0;
        while s < len - 1.0 {
            let (p, d, right) = at(s, -1.0, back_off);
            // Wooden poles stand in the ground, not on a bridge's or
            // viaduct's deck; the wires span it to the next pole.
            let on_deck = run.nodes.windows(2).filter_map(|w| segment_between(roads, w[0], w[1])).any(|sid| {
                let sg = roads.segment(sid);
                let (a, b) = roads.segment_points(sid);
                (sg.bridge || sg.viaduct) && nd_sim::roads::project_on_segment(p, a, b).1.distance(p) < hw + SIDEWALK + 0.5
            });
            if !free(p) || !kerbed(s, -1.0) || on_deck {
                s += 32.0;
                continue;
            }
            g.part(Wood).cylinder(p, 0.16, 0.12, 0.0, 8.5, 6, rgba(pal.bark));
            g.part(Wood).boxed(Vec3::new(p.x, 8.0, p.y), Vec3::new(0.1, 0.1, 1.6), d.y.atan2(d.x), rgba(pal.bark));
            let top = Vec3::new(p.x, 8.2, p.y);
            if let Some(q) = prev {
                for dz in [-0.6f32, 0.0, 0.6] {
                    let off = Vec3::new(right.x, 0.0, right.y) * dz;
                    g.part(Matte).wire(q + off, top + off, 0.8, 0.035, rgba(DARK));
                }
            }
            if rng.random_bool(0.3) {
                g.part(Metal).boxed(Vec3::new(p.x, 7.0, p.y) + Vec3::new(right.x, 0.0, right.y) * 0.35, Vec3::new(0.5, 0.9, 0.5), 0.0, rgba(mul(pal.steel, 0.6)));
            }
            prev = Some(top);
            s += 32.0;
        }
    }
    let mut s = 10.0 + rng.random_range(0.0..8.0);
    while s < len - 6.0 {
        let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let (p, d, right) = at(s, side, hw + 1.2);
        let pick = rng.random_range(0..6);
        if !free(p) || !free(at(s, side, back_off).0) || !kerbed(s, side) {
            s += 22.0 + rng.random_range(0.0..20.0);
            continue;
        }
        match pick {
            0 | 1 => bin_world(g, p, mul(pal.metal_paint(0), 0.9)),
            2 if lv_mid > 0.35 => {
                let (bp, _, _) = at(s, side, back_off);
                bench_world(g, bp, d.y.atan2(d.x) + if side > 0.0 { std::f32::consts::PI } else { 0.0 }, pal.wood, CHARCOAL);
            }
            3 if lv_mid < 0.55 => {
                let (kp, _, _) = at(s, side, hw + 1.4);
                let yaw = d.y.atan2(d.x);
                g.part(Metal).boxed(Vec3::new(kp.x, 1.2, kp.y), Vec3::new(1.0, 2.4, 0.9), yaw, rgba(pal.metal_paint(2)));
                let face = kp - right * side * 0.47;
                let out = Vec3::new(-right.x, 0.0, -right.y) * side;
                g.part(Glow).decal(Vec3::new(face.x, 1.5, face.y), Vec3::new(d.x, 0.0, d.y), Vec3::Y, 0.6, 0.8, out, rgba(mul(pal.led(1), 0.5)));
            }
            4 if len > 80.0 => {
                // A phone box where a shelter used to stand at random;
                // shelters now mark real bus stops.
                let (sp, d, _) = at(s, side, hw + SIDEWALK * 0.55);
                let yaw = d.y.atan2(d.x);
                g.part(Glass).boxed(Vec3::new(sp.x, 1.2, sp.y), Vec3::new(1.0, 2.4, 1.0), yaw, rgba(pal.glass(0)));
                g.part(Panel).boxed(Vec3::new(sp.x, 2.45, sp.y), Vec3::new(1.1, 0.1, 1.1), yaw, rgba(mul(pal.steel, 0.8)));
            }
            _ => {}
        }
        s += 22.0 + rng.random_range(0.0..20.0);
    }
}

/// Roundabouts: a mountable apron inside the ring lanes; the middle is
/// open land. At the height of the ring's nodes.
pub(super) fn draw_roundabout_aprons(cx: &StreetCtx, g: &mut Geometry) {
    let roads = cx.roads;
    for ring in roads.rings.iter().filter(|_| cx.inp.layer != StreetLayer::Underground) {
        let from = marks(g);
        roundabout_apron(g, cx.pal, ring);
        if !cx.heights.is_empty() {
            let on: Vec<f32> = roads.nodes.iter().enumerate().filter(|(_, n)| ring.on_circle(n.pos)).map(|(i, _)| cx.node_z(NodeId(i as u32))).collect();
            let z = if on.is_empty() { 0.0 } else { on.iter().sum::<f32>() / on.len() as f32 };
            lift_since(g, &from, |_| z);
        }
    }
}

/// Junction pads, crossings and stop lines (bends need none), each at its
/// node's height.
pub(super) fn draw_nodes(cx: &StreetCtx, g: &mut Geometry) {
    for (i, node) in cx.roads.nodes.iter().enumerate() {
        let nid = NodeId(i as u32);
        let from = (!cx.heights.is_empty()).then(|| (marks(g), cx.node_z(nid)));
        draw_node(cx, g, nid, node);
        if let Some((from, z)) = from {
            if z != 0.0 {
                lift_since(g, &from, |_| z);
            }
        }
    }
}

pub(super) fn draw_node(cx: &StreetCtx, g: &mut Geometry, nid: NodeId, node: &nd_sim::roads::Node) {
    let (pal, roads) = (cx.pal, cx.roads);
    if !cx.inp.layer.keeps(tunnel_node(roads, nid)) {
        return;
    }
    let alive: Vec<SegmentId> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
    let paths_here = node.segments.iter().filter(|&&s| roads.segment(s).alive && roads.segment(s).kind == RoadKind::Path).count();
    let rails_here: Vec<SegmentId> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_rail()).collect();
    if !alive.is_empty() && !rails_here.is_empty() {
        level_crossing(g, pal, roads, nid, node.pos, &alive, &rails_here);
        return;
    }
    if alive.len() == 2 && paths_here > 0 {
        // A path crossing a street: zebra across the carriageway (none
        // where a footbridge replaced it).
        if crosswalk_closed(cx.inp.closed, node.pos) {
            return;
        }
        let seg = roads.segment(alive[0]);
        let (a, b) = roads.segment_points(alive[0]);
        let d = (b - a).normalize_or_zero();
        zebra(g, pal, node.pos - d * 1.3, node.pos + d * 1.3, Vec2::new(-d.y, d.x), seg.half_width(), |_| false);
        return;
    }
    if alive.len() == 2 {
        // Where a road carries on with fewer or more lanes (a ramp onto
        // a highway's end, a street that widens), the carriageway
        // narrows over a taper instead of stepping at the joint.
        if let Some((_, ri, path, hw0, hw1)) = cx.tapers.iter().find(|t| t.0 == nid) {
            let (ri, hw0, hw1) = (*ri, *hw0, *hw1);
            let from = marks(g);
            let motorway = alive.iter().any(|&s| roads.segment(s).is_motorway());
            lane_taper(g, path, hw0, hw1, cx.asphalt_c, cx.edge_c, motorway.then(|| rgba(mul(pal.concrete(0), 0.8))));
            if !cx.lifts.is_empty() {
                // On the narrower run's profile (the node pass adds the
                // node's height to all it draws).
                let z0 = cx.node_z(nid);
                lift_since(g, &from, |p| cx.run_z(ri, p) - z0);
            }
        }
        return;
    }
    if alive.is_empty() {
        return;
    }
    // A ramp meeting a highway: a taper and a hatched gore instead of a
    // junction pad, crossings and stop lines.
    if let Some(ramp) = gore_ramp(roads, nid) {
        // Bright paint, as the lane arrows: the hatching is the one
        // marking that tells a driver where not to be.
        let (v, ix) = (g.get(Asphalt).positions.len(), g.get(Asphalt).indices.len());
        ramp_gore(g, roads, nid, ramp, cx.asphalt_c, rgba(mul(pal.lane_marker, 1.25)));
        // In the same asphalt as the carriageway it widens out of (a
        // highway out of town is worn; the gore was a pale patch on it).
        let carriageway = gore_lines(roads, nid, ramp).and_then(|_| node.segments.iter().copied().find(|&s| s != ramp && roads.segment(s).alive && roads.segment(s).is_highway()));
        if carriageway.and_then(|s| cx.arm_run.get(&(nid.0, s.0))).is_some_and(|&ri| cx.run_worn[ri]) {
            weather_tail(g, vec![(Asphalt, v, ix)]);
        }
        return;
    }
    let Some(arms) = cx.pad_mouths.get(&nid.0) else { return };
    draw_pad(cx, g, nid, node, arms);
    draw_arm_marks(cx, g, nid, node, arms);
}

/// A junction's pad: the polygon through the arms' mouths and the corners'
/// kerb lines (`junction::pad_outline`), each point at the height of the
/// road beside it (the node pass adds the node's own), worn like the
/// corners at a cheap junction.
pub(super) fn draw_pad(cx: &StreetCtx, g: &mut Geometry, nid: NodeId, node: &nd_sim::roads::Node, arms: &[Mouth]) {
    let i = nid.0;
    let node_corners = cx.corners.by_node.get(&i);
    let corner_x = |sa: SegmentId, sb: SegmentId| node_corners.into_iter().flatten().find(|c| c.arms[0].2 == sa && c.arms[1].2 == sb && c.turn < 0.0 && !c.apron.is_empty()).map(|c| c.apron[0]);
    let outline = junction::pad_outline_with(node.pos, arms, &corner_x);
    if outline.len() < 3 {
        return;
    }
    let z0 = cx.node_z(nid);
    let local = |pp: &junction::PadPoint| {
        let rz = |k: usize| cx.arm_run.get(&(i, arms[k].seg.0)).map_or(z0, |&ri| cx.run_z(ri, pp.p));
        if cx.lifts.is_empty() {
            0.0
        } else if pp.arms[0] == pp.arms[1] {
            rz(pp.arms[0]) - z0
        } else {
            rz(pp.arms[0]) + rz(pp.arms[1]) - 2.0 * z0
        }
    };
    let ring: Vec<Vec3> = outline.iter().map(|pp| Vec3::new(pp.p.x, ASPHALT_Y + 0.01 + local(pp), pp.p.y)).collect();
    let worn = cx.inp.land_value.at(node.pos) < 0.35;
    let (v, ix) = (g.get(Asphalt).positions.len(), g.get(Asphalt).indices.len());
    g.part(Asphalt).ground_fan(Vec3::new(node.pos.x, ASPHALT_Y + 0.01, node.pos.y), &ring, cx.asphalt_c);
    if worn {
        weather_tail(g, vec![(Asphalt, v, ix)]);
    }
}

/// A junction's arms: the zebra where the walk graph's crosswalk is and a
/// stop line behind it, or on a roundabout the arm's splitter island and
/// give-way line.
pub(super) fn draw_arm_marks(cx: &StreetCtx, g: &mut Geometry, nid: NodeId, node: &nd_sim::roads::Node, arms: &[Mouth]) {
    let (pal, roads) = (cx.pal, cx.roads);
    let i = nid.0 as usize;
    let on_ring = roads.ring_of_node(nid).is_some();
    let r = cx.net.node_radius.get(i).copied().unwrap_or(4.0) + 0.3;
    for m in arms {
        let sid = m.seg;
        let seg = roads.segment(sid);
        let d = m.dir;
        let right = Vec2::new(-d.y, d.x);
        let hw = seg.half_width();
        if on_ring {
            // No crossing on the ring itself; arms get theirs set back
            // past a splitter island, and a give-way line at the mouth.
            if roads.ring_of_segment(sid).is_none() {
                roundabout_arm(g, pal, node.pos, d, r, seg, seg.a.0 as usize == i);
            }
            continue;
        }
        // The zebra where the walk graph's crosswalk is (and the kerbs
        // drop): it used to be painted at the junction's radius whatever
        // the set-back, so at a sharp corner the paint and the curb cut
        // were metres apart and two arms' zebras overlapped.
        // [footbridge] None where a footbridge replaced the crosswalk:
        // the stop line moves up to the junction's mouth.
        let stop = if junction::has_crosswalk(roads, nid, sid) && !crosswalk_closed(cx.inp.closed, node.pos + d * nd_sim::walk::crosswalk_setback(roads, nid, sid)) {
            let mid = nd_sim::walk::crosswalk_setback(roads, nid, sid);
            let half = nd_sim::walk::CROSSWALK_HALF;
            zebra(g, pal, node.pos + d * (mid - half), node.pos + d * (mid + half), right, hw, |_| false);
            mid + half + 0.5
        } else {
            m.at + 0.5
        };
        let incoming = if seg.a.0 as usize == i { seg.lanes_bwd } else { seg.lanes_fwd } as f32;
        if incoming > 0.0 {
            let w = incoming * LANE_WIDTH;
            let centre = node.pos + d * stop - right * (w * 0.5);
            g.part(Paint).ground_strip(&[centre - right * w * 0.5, centre + right * w * 0.5], 0.4, MARK_Y + 0.002, rgba(mul(pal.lane_marker, 1.1)));
        }
    }
}

/// Junction corners: a rounded kerb from one arm's pavement round to the
/// next, the paving behind it out to where the arms' pavements end, and
/// asphalt out to where the kerb lines would have met, so no bare ground
/// shows between the arms. The kerb drops where the arms' crosswalks cross
/// it. Each corner is set on the heights of the two runs it joins, blended
/// along it; a cheap junction's corners are worn like its streets.
pub(super) fn draw_corners(cx: &StreetCtx, g: &mut Geometry) {
    let (pal, roads) = (cx.pal, cx.roads);
    for (&node, list) in cx.corners.by_node.iter().filter(|_| cx.inp.layer.keeps(false)) {
        let nid = NodeId(node);
        let centre = roads.node(nid).pos;
        let worn = worn_marks(g);
        for corner in list {
            let from = marks(g);
            let corner_cuts: Vec<Cut> = corner.arms.iter().flat_map(|a| cx.cuts.get(&a.2 .0).into_iter().flatten().copied()).collect();
            kerb_ribbon(g, &corner.kerb, Some(&corner.normals), -1.0, CORNER_RIBBON, false, corner.stub, &corner_cuts, &cx.kerb_cols);
            if corner.apron.len() >= 3 {
                g.part(Asphalt).ground_polygon(&corner.apron, ASPHALT_Y + 0.01, cx.asphalt_c);
            }
            if corner.fill.len() >= 3 {
                corner_fill(g, corner, centre, cx.paving_c);
            }
            // Bollards on the corner of a junction proper, behind the kerb
            // where turning lorries cut it.
            if roads.street_degree(nid) >= 3 && corner.turn < -0.5 {
                let kerb = Polyline::new(&corner.kerb);
                let len = path_length(&corner.kerb);
                for ds in [-0.7f32, 0.7] {
                    let (p, d) = kerb.at(len * 0.5 + ds);
                    bollard_world(g, p + Vec2::new(d.y, -d.x) * 0.55, mul(pal.steel, 0.7));
                }
            }
            if !cx.lifts.is_empty() {
                // Each run's profile is the node's height at the junction and
                // rises along the run, so the two together are the height of
                // the road beside either arm (the other contributing only
                // the node's), and a plane through both slopes between.
                let (ra, rb) = (corner.arms[0].0, corner.arms[1].0);
                let z0 = cx.node_z(nid);
                lift_since(g, &from, |p| cx.run_z(ra, p) + cx.run_z(rb, p) - z0);
            }
        }
        if cx.inp.land_value.at(centre) < 0.35 {
            weather_tail(g, worn);
        }
    }
}

/// The paving filling a junction corner behind its kerb ribbon, and its
/// edges down to the ground.
pub(super) fn corner_fill(g: &mut Geometry, corner: &Corner, centre: Vec2, paving_c: [f32; 4]) {
    // 3 mm over the pavements it overlaps (theirs at KERB_H -
    // 0.01, with other UVs): one surface wins, no flicker.
    g.part(Paving).ground_polygon(&corner.fill, CORNER_FILL_Y, paving_c);
    let mut edges: Vec<(Vec2, Vec2, Option<Vec2>)> = corner.rim.windows(2).map(|w| (w[0], w[1], None)).collect();
    if corner.stub && !corner.rim.is_empty() {
        // Where the paving stops beside a link with none: its
        // two ends (one of them hidden under the other arm's).
        let (k0, k1) = (corner.kerb[0], corner.kerb[corner.kerb.len() - 1]);
        let back_last = corner.fill[corner.fill.len() - corner.rim.len() - 1];
        edges.push((back_last, corner.rim[0], Some(k1 - corner.kerb[corner.kerb.len() - 2])));
        edges.push((corner.rim[corner.rim.len() - 1], corner.fill[0], Some(k0 - corner.kerb[1])));
    }
    for (p, q, facing) in edges {
        let n = Vec2::new(-(q - p).y, (q - p).x);
        let towards = facing.unwrap_or((p + q) * 0.5 - centre);
        let n = if n.dot(towards) < 0.0 { -n } else { n };
        let y = CORNER_FILL_Y;
        g.part(Paving).quad_facing([Vec3::new(p.x, 0.0, p.y), Vec3::new(q.x, 0.0, q.y), Vec3::new(q.x, y, q.y), Vec3::new(p.x, y, p.y)], Vec3::new(n.x, 0.0, n.y), paving_c);
    }
}

/// [pedestrians] Mid-block zebras on long blocks, with a beacon post at
/// each kerb.
pub(super) fn draw_midblock(cx: &StreetCtx, g: &mut Geometry) {
    let (pal, roads) = (cx.pal, cx.roads);
    for (sid, t) in nd_sim::walk::midblock_crossings(roads) {
        let seg = roads.segment(sid);
        // Each layer draws only its own roads' crossings: these used to be
        // drawn in every layer, so the underground view showed zebras and
        // beacons floating over the surface streets.
        if !cx.inp.layer.keeps(seg.tunnel) {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let d = (b - a).normalize_or_zero();
        let right = Vec2::new(-d.y, d.x);
        let hw = seg.half_width();
        let at = a.lerp(b, t);
        if crosswalk_closed(cx.inp.closed, at) {
            continue;
        }
        // [elevation] At the road's own height along the segment.
        let z = nd_sim::landscape::way_height(roads, cx.heights, cx.inp.inner, sid, t);
        at_height(g, z, |g| {
            zebra(g, pal, at - d * nd_sim::walk::CROSSWALK_HALF, at + d * nd_sim::walk::CROSSWALK_HALF, right, hw, |_| false);
            for side in [-1.0f32, 1.0] {
                let p = at + right * side * (hw + 0.45) + d * (nd_sim::walk::CROSSWALK_HALF + 0.3);
                g.part(Matte).cylinder(p, 0.06, 0.05, 0.0, 2.6, 6, rgba(CHARCOAL));
                g.part(Glow).boxed(Vec3::new(p.x, 2.75, p.y), Vec3::splat(0.3), 0.0, rgba(pal.beacon));
            }
        });
    }
}

/// [pedestrians] Walk / don't walk heads on signalised crosswalks: a short
/// pole at each kerb with a head facing across.
pub(super) fn draw_ped_signals(cx: &StreetCtx, g: &mut Geometry) {
    let (roads, net) = (cx.roads, cx.net);
    for post in ped_signal_posts_at(roads, net, &cx.corners).into_iter().filter(|p| !crosswalk_closed(cx.inp.closed, p.crosswalk)) {
        // The junction the crosswalk belongs to decides the layer and the
        // height (not the nearest node, which may be another road's).
        let Some(node) = net.intersections.get(post.inter).map(|i| i.node) else { continue };
        if !cx.inp.layer.keeps(tunnel_node(roads, node)) {
            continue;
        }
        at_height(g, cx.node_z(node), |g| {
            g.part(Matte).cylinder(post.pole, 0.07, 0.06, 0.0, PED_HEAD_Y + 0.4, 6, rgba(CHARCOAL));
            let f = Vec2::from_angle(post.heading);
            let head = Vec3::new(post.pole.x + f.x * 0.12, PED_HEAD_Y, post.pole.y + f.y * 0.12);
            g.part(Matte).boxed(head, Vec3::new(0.34, 0.62, 0.34), 0.0, rgba(DARK));
        });
    }
}

/// Signal posts: pole at the kerb, arm over the approach, one head per lane.
pub(super) fn draw_signals(cx: &StreetCtx, g: &mut Geometry) {
    let roads = cx.roads;
    let heights = cx.heights;
    for post in signal_groups(roads, cx.net, &cx.corners) {
        // Layer and height from the junction the approach feeds: two
        // nearest-node scans per post were O(nodes) each, and a node with
        // no live roads counted as underground (`all` of nothing).
        if !cx.inp.layer.keeps(roads.nodes.get(post.node.0 as usize).is_some_and(|_| tunnel_node(roads, post.node))) {
            continue;
        }
        // [interchange] The arm keeps its clearance over the lanes it hangs
        // over: on an approach climbing to the junction from below (or
        // falling to it) the road under the heads stands higher than the
        // junction the post is set on, and the arm hung 3.2 m over it.
        let z = cx.node_z(post.node);
        let rise = if heights.is_empty() {
            0.0
        } else {
            let (sa, sb) = roads.segment_points(post.seg);
            let len = sa.distance(sb).max(1e-3);
            post.heads
                .iter()
                .chain(std::iter::once(&post.leftmost))
                .map(|&h| {
                    let t = nd_sim::roads::project_on_segment(h, sa, sb).0;
                    nd_sim::landscape::way_height(roads, heights, cx.inp.inner, post.seg, t) - z
                })
                .fold(0.0f32, f32::max)
                .min(len)
        };
        at_height(g, z, |g| {
            let base = post.pole;
            let pole_h = 5.6 + rise;
            g.part(Matte).cylinder(base, 0.16, 0.12, 0.0, pole_h, 8, rgba(CHARCOAL));
            let a = Vec3::new(base.x, pole_h - 0.2, base.y);
            let b = Vec3::new(post.leftmost.x, pole_h - 0.2, post.leftmost.y);
            g.part(Matte).beam(a, b, 0.18, rgba(CHARCOAL));
            for lamp in &post.heads {
                let head = Vec3::new(lamp.x, 4.75 + rise, lamp.y);
                g.part(Matte).boxed(head, Vec3::new(0.35, 1.1, 0.4), -post.heading, rgba(DARK));
                g.part(Matte).beam(Vec3::new(lamp.x, pole_h - 0.2, lamp.y), head + Vec3::Y * 0.55, 0.08, rgba(CHARCOAL));
            }
        });
    }
}

/// The inner edge of a roundabout: a low, mountable cobbled apron on the
/// shoulder inside the ring lanes, for long vehicles to cut over. The
/// ground inside the ring is left open, as land like any other.
pub(super) fn roundabout_apron(g: &mut Geometry, pal: &Palette, ring: &nd_sim::roads::Ring) {
    use std::f32::consts::TAU;
    let r = ring.radius - ring.lanes as f32 * LANE_WIDTH * 0.5 - SHOULDER * 0.5;
    let n = ((TAU * r / 3.0) as usize).clamp(24, 256);
    let mut edge: Vec<Vec2> = (0..n).map(|k| ring.center + Vec2::from_angle(k as f32 / n as f32 * TAU) * r).collect();
    edge.push(edge[0]);
    g.part(Paving).path_box(&edge, 0.0, SHOULDER - 0.2, 0.0, ASPHALT_Y + 0.06, rgba(mul(pal.paving, 0.85)));
}

/// Where an arm meets a roundabout: a splitter island down the middle of a
/// two-way arm, a zebra set back from the ring across it (with a refuge in
/// the island), and a dashed give-way line across the lanes going in.
pub(super) fn roundabout_arm(g: &mut Geometry, pal: &Palette, at: Vec2, d: Vec2, r: f32, seg: &nd_sim::Segment, from_a: bool) {
    let right = Vec2::new(-d.y, d.x);
    let hw = seg.half_width();
    let (outgoing, incoming) = if from_a { (seg.lanes_fwd, seg.lanes_bwd) } else { (seg.lanes_bwd, seg.lanes_fwd) };
    let total = (outgoing + incoming) as f32;
    // The boundary between the directions, as a lateral offset (right of
    // `d` positive): lanes going out keep right, lanes coming in are left.
    let divide = (incoming as f32 - total * 0.5) * LANE_WIDTH;
    let (z0, z1) = (r + 5.0, r + 7.4);
    if outgoing > 0 && incoming > 0 {
        for (s0, s1, w0) in [(r + 0.9, z0 - 0.3, 0.5), (z1 + 0.3, r + 12.0, 0.9)] {
            let a = at + d * s0 + right * divide;
            let b = at + d * s1 + right * divide;
            g.part(Concrete).path_box(&[a, b], 0.0, w0, 0.0, KERB_H, rgba(pal.curb));
            g.part(Paint).path_strip(&[a - d * 0.6, b + d * 0.6], 0.0, w0 + 0.5, MARK_Y, rgba(mul(pal.lane_marker, 0.5)));
        }
    }
    zebra(g, pal, at + d * z0, at + d * z1, right, hw, |s| !((s - divide).abs() > 0.6 || outgoing == 0 || incoming == 0));
    if incoming > 0 {
        let (l0, l1) = (-(hw - SHOULDER), divide);
        let mut x = l0 + 0.3;
        while x + 0.6 < l1 {
            let p = at + d * (r + 0.9) + right * x;
            g.part(Paint).ground_strip(&[p, p + right * 0.6], 0.35, MARK_Y + 0.003, rgba(mul(pal.lane_marker, 1.1)));
            x += 1.1;
        }
    }
}

/// Neglect on a cheap street: potholes as dark patches on the carriageway,
/// cracks across the pavement slabs and litter blown up against the kerb.
/// The strips themselves are untouched; this only lays thin things on top.
pub(super) fn street_grit(g: &mut Geometry, pal: &Palette, walk: &[Vec2], hw: f32, len: f32, rng: &mut impl Rng) {
    let pothole = rgba(mul(pal.asphalt, 0.5));
    let crack = rgba(mul(pal.paving, 0.35));
    let at = |s: f32| {
        let (p, d) = point_along(walk, s);
        (p, d, Vec2::new(-d.y, d.x))
    };
    // Potholes.
    let lane_room = hw - SHOULDER - 0.7;
    if lane_room > 0.2 {
        let mut s = rng.random_range(3.0..12.0);
        while s < len - 2.0 {
            let (p, _, right) = at(s);
            let c = p + right * rng.random_range(-lane_room..lane_room);
            let r = rng.random_range(0.3..0.8);
            let rot = rng.random_range(0.0..std::f32::consts::TAU);
            let squash = rng.random_range(0.6..1.0);
            let poly: Vec<Vec2> = (0..7)
                .map(|k| {
                    let a = k as f32 / 7.0 * std::f32::consts::TAU;
                    let (x, y) = (a.cos() * r, a.sin() * r * squash);
                    c + Vec2::new(x * rot.cos() - y * rot.sin(), x * rot.sin() + y * rot.cos())
                })
                .collect();
            g.part(Asphalt).ground_polygon(&poly, ASPHALT_Y + 0.006, pothole);
            s += rng.random_range(9.0..22.0);
        }
    }
    // Cracks in the pavement slabs, with a branch now and then.
    let mut s = 2.0;
    while s < len - 1.0 {
        for side in [-1.0f32, 1.0] {
            if !rng.random_bool(0.55) {
                continue;
            }
            let (p, d, right) = at(s);
            let yaw = d.y.atan2(d.x) + rng.random_range(-0.6..0.6);
            let c = p + right * side * (hw + 0.3 + rng.random_range(0.4..SIDEWALK - 0.8));
            let l = rng.random_range(0.6..2.2);
            g.part(Matte).boxed(Vec3::new(c.x, KERB_H - 0.005, c.y), Vec3::new(l, 0.012, 0.05), yaw, crack);
            if rng.random_bool(0.5) {
                let b = c + Vec2::new(yaw.cos(), yaw.sin()) * l * 0.3;
                g.part(Matte).boxed(Vec3::new(b.x, KERB_H - 0.005, b.y), Vec3::new(l * 0.5, 0.012, 0.05), yaw + rng.random_range(0.6..1.2), crack);
            }
        }
        s += rng.random_range(4.0..9.0);
    }
    // Litter against the kerb: bags, cans, sheets of plastic, cardboard.
    let mut s = 1.0;
    while s < len - 1.0 {
        let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let (p, d, right) = at(s);
        let c = p + right * side * (hw + rng.random_range(0.35..1.1));
        let yaw = d.y.atan2(d.x) + rng.random_range(0.0..std::f32::consts::TAU);
        let base = KERB_H - 0.01;
        match rng.random_range(0..4) {
            0 => g.part(Tarp).boxed(Vec3::new(c.x, base + 0.14, c.y), Vec3::new(0.4, 0.28, 0.32), yaw, rgba(pal.litter_bag)),
            1 => g.part(Matte).boxed(Vec3::new(c.x, base + 0.06, c.y), Vec3::new(0.1, 0.12, 0.1), yaw, rgba(mul(pal.paving, 1.4))),
            2 => g.part(Tarp).boxed(Vec3::new(c.x, base + 0.01, c.y), Vec3::new(0.5, 0.02, 0.4), yaw, rgba(mul(pick(rng, &pal.tarp_colors), 0.8))),
            _ => g.part(Wood).boxed(Vec3::new(c.x, base + 0.025, c.y), Vec3::new(0.6, 0.05, 0.45), yaw, rgba(mul(pal.plywood, 0.75))),
        }
        s += rng.random_range(5.0..14.0);
    }
}
