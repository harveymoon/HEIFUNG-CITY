//! Railway track, level crossings and footpaths: the runs that are not streets.

use super::*;

/// Single track: ballast, sleepers, two rails, drawn by the same hand as a
/// freight yard's sidings.
pub(super) fn railway(g: &mut Geometry, pal: &Palette, roads: &RoadNetwork, run: &Run, points: &[Vec2]) {
    // Stopped at a level crossing's edge, where `level_crossing` lays the
    // rails into the road: the ballast, sleepers and rails used to run on
    // over the asphalt and the kerbs, standing 0.3 m proud of the road.
    // [pt-visual] The same where only a footpath crosses: the track stops at
    // the path's edges and `path_rail_crossing` lays the rails over it.
    let (t0, t1) = (crossing_reach(roads, run.start_node, points, true, RAIL_HALF_WIDTH), crossing_reach(roads, run.end_node, points, false, RAIL_HALF_WIDTH));
    let pts = if t0 == 0.0 && t1 == 0.0 { points.to_vec() } else { trim(points, t0, t1) };
    railway_track(g, pal, &pts, RAIL_HALF_WIDTH * 2.0);
}

/// How far along a path or track from its end at `node` it stays back for
/// the streets crossing there: out to the back of their pavements, measured
/// along it (further at a slant). Zero where no street meets it.
pub(super) fn crossing_reach(roads: &RoadNetwork, node: NodeId, points: &[Vec2], start: bool, half: f32) -> f32 {
    crossing_reach_to(roads, node, points, start, half, false)
}

/// [`crossing_reach`], or (`to_kerb`, for a footpath) out to the kerb lines
/// only: see below.
pub(super) fn crossing_reach_to(roads: &RoadNetwork, node: NodeId, points: &[Vec2], start: bool, half: f32, to_kerb: bool) -> f32 {
    if roads.street_degree(node) == 0 || points.len() < 2 {
        return 0.0;
    }
    let n = points.len();
    let d = if start { points[1] - points[0] } else { points[n - 2] - points[n - 1] }.normalize_or_zero();
    roads
        .node(node)
        .segments
        .iter()
        .filter(|&&s| roads.segment(s).alive && roads.segment(s).is_street())
        .map(|&s| {
            let (a, b) = roads.segment_points(s);
            let sg = roads.segment(s);
            if !to_kerb {
                // Track: out to the back of the pavements, where the
                // level crossing's rails end.
                let along = (b - a).normalize_or_zero();
                let sin = d.perp_dot(along).abs().max(0.35);
                // Its own width's corners reach further at a slant.
                return (sg.half_width() + SIDEWALK + half * d.dot(along).abs()) / sin;
            }
            // The street's way out of the node.
            let along = if sg.a == node { b - a } else { a - b }.normalize_or_zero();
            let sin = d.perp_dot(along).abs();
            // [pt-walk] F43: a path runs out to the street's kerb line, not
            // the back of its pavement: the pavement (drawn higher) covers
            // it where there is one, and where the junction's rounded
            // corner has none it runs on to the kerb instead of stopping
            // in the dirt metres short. A street behind the path (one that
            // ends where the path carries on) is left at its kerb across
            // the path's way.
            let hw = sg.half_width();
            if d.dot(along) <= 0.0 {
                return hw * sin + half * (1.0 - sin);
            }
            (hw + 0.3 + half * d.dot(along).abs()) / sin.max(0.35)
        })
        .fold(0.0, f32::max)
}

/// [pt-visual] Where a footpath crosses the track with no street: a dark
/// rubber crossing panel at the rails' height over the ballast and the
/// sleepers, the rails flush in it, ramps down to the path either side and
/// yellow tactile strips; the path's paving used to run flat across the
/// ballast between the sleepers. The gates and lamps are the app's
/// (`crate::crossing`).
pub(super) fn path_rail_crossing(g: &mut Geometry, pal: &Palette, roads: &RoadNetwork, at: Vec2, paths: &[SegmentId], rails: &[SegmentId]) {
    let Some(&rail) = rails.first() else { return };
    let (ra, rb) = roads.segment_points(rail);
    let rd = (ra - rb).normalize_or_zero();
    let rn = Vec2::new(-rd.y, rd.x);
    for &path in paths {
        let (pa, pb) = roads.segment_points(path);
        let pd = (pb - pa).normalize_or_zero();
        let p_right = Vec2::new(-pd.y, pd.x);
        let (cu, cv) = (rd.dot(p_right), rn.dot(p_right));
        if cu.abs() <= 0.2 {
            continue;
        }
        let at_lat = |lat: f32, v: f32| at + rd * ((lat - v * cv) / cu) + rn * v;
        let hw = PATH_HALF_WIDTH + 0.1;
        let half = RAIL_HALF_WIDTH.min(1.6);
        // The panel stands at the rails' height over the ballast and the
        // sleepers (the track runs on under it), the path ramping up to it.
        const TOP: f32 = 0.35;
        let rubber = rgba([0.07, 0.07, 0.08]);
        let panel = [at_lat(-hw, -half), at_lat(hw, -half), at_lat(hw, half), at_lat(-hw, half)];
        g.part(Matte).ground_polygon(&panel, TOP, rubber);
        let v3 = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
        for sgn in [-1.0f32, 1.0] {
            let (e, f) = (half * sgn, (half + 1.2) * sgn);
            let q = [v3(at_lat(-hw, e), TOP), v3(at_lat(hw, e), TOP), v3(at_lat(hw, f), ASPHALT_Y + 0.02), v3(at_lat(-hw, f), ASPHALT_Y + 0.02)];
            g.part(Matte).quad_facing(q, Vec3::Y, rubber);
            // The panel's sides down to the ground.
            for lat in [-hw, hw] {
                let (a, b) = (at_lat(lat, -half), at_lat(lat, half));
                let out = (at_lat(lat * 2.0, 0.0) - at_lat(0.0, 0.0)).normalize_or_zero();
                g.part(Matte).quad_facing([v3(a, 0.0), v3(b, 0.0), v3(b, TOP), v3(a, TOP)], Vec3::new(out.x, 0.0, out.y), rubber);
            }
        }
        for v in [-0.72f32, 0.72] {
            g.part(Metal).ground_strip(&[at_lat(-hw, v), at_lat(hw, v)], 0.08, TOP + 0.02, rgba(mul(pal.steel, 0.75)));
        }
        // Tactile strips on the paving either side, yellow.
        for v in [-(half + 1.7), half + 1.7] {
            g.part(Paint).ground_strip(&[at_lat(-PATH_HALF_WIDTH, v), at_lat(PATH_HALF_WIDTH, v)], 0.4, ASPHALT_Y + 0.03, rgba([0.85, 0.7, 0.1]));
        }
    }
}

/// Where track crosses a street: planks between the rails over the
/// carriageway, crossbuck signs and barrier posts on both sides.
pub(super) fn level_crossing(g: &mut Geometry, pal: &Palette, roads: &RoadNetwork, node: NodeId, at: Vec2, streets: &[SegmentId], rails: &[SegmentId]) {
    let Some(&street) = streets.first() else { return };
    let Some(&rail) = rails.first() else { return };
    let (sa, sb) = roads.segment_points(street);
    let sd = (sb - sa).normalize_or_zero();
    let s_right = Vec2::new(-sd.y, sd.x);
    let (ra, rb) = roads.segment_points(rail);
    let rd = (ra - rb).normalize_or_zero();
    let hw = roads.segment(street).half_width();
    // The rails laid flush into the road: a crossing panel between and
    // beside them over the carriageway, cut square to the kerb lines (the
    // planks used to float 15 cm over the asphalt and run out over the
    // pavements), and the two rails over it and across the pavements at
    // their height. The track itself stops at the back of the pavements
    // (`railway`).
    let rn = Vec2::new(-rd.y, rd.x);
    let (cu, cv) = (rd.dot(s_right), rn.dot(s_right));
    if cu.abs() > 0.2 {
        // The point on the rail's line `v` across it whose offset from the
        // street's centre line is `lat`.
        let at_lat = |lat: f32, v: f32| at + rd * ((lat - v * cv) / cu) + rn * v;
        let half = RAIL_HALF_WIDTH.min(1.6);
        let panel = [at_lat(-hw, -half), at_lat(hw, -half), at_lat(hw, half), at_lat(-hw, half)];
        g.part(Wood).ground_polygon(&panel, MARK_Y + 0.001, rgba(mul(pal.wood, 0.45)));
        for v in [-0.72f32, 0.72] {
            g.part(Metal).ground_strip(&[at_lat(-hw, v), at_lat(hw, v)], 0.08, MARK_Y + 0.006, rgba(mul(pal.steel, 0.75)));
            for side in [-1.0f32, 1.0] {
                g.part(Metal).ground_strip(&[at_lat(side * hw, v), at_lat(side * (hw + SIDEWALK), v)], 0.08, KERB_H - 0.004, rgba(mul(pal.steel, 0.75)));
            }
        }
    }
    // [rail-crossing] Stop lines across the arriving lanes, clear of the
    // track (the lanes end there: `nd_sim::level_crossing::stop_radius`).
    // The posts, lamps and barriers are the app's (`crate::crossing`).
    if let Some(c) = nd_sim::level_crossing::at(roads, node) {
        for a in c.arms.iter().filter(|a| !a.path && a.lanes_in > 0) {
            let h = a.heading();
            let right = Vec2::new(-h.y, h.x);
            let total = (a.lanes_in + a.lanes_out) as f32;
            let p = at + a.away * (a.stop + 1.0);
            let (from, to) = (-total * 0.5 * LANE_WIDTH + a.lanes_out as f32 * LANE_WIDTH, total * 0.5 * LANE_WIDTH);
            let from = if a.lanes_out > 0 { from + 0.15 } else { from };
            g.part(Paint).ground_strip(&[p + right * from, p + right * to], 0.45, MARK_Y + 0.003, rgba(mul(pal.lane_marker, 1.0)));
        }
    }
}

/// A path for walkers and bikes: paving with edge stones, low lamps, the odd
/// bench and bin, and bollards where it meets a street so cars stay out.
pub(super) fn footpath(g: &mut Geometry, pal: &Palette, roads: &RoadNetwork, run: &Run, points: &[Vec2], rng: &mut impl Rng, profile: Option<&RunProfile>) {
    // Where the path meets a street, stop at the back of that street's
    // pavement and flare out onto it, instead of running under the kerb.
    let mut points = points.to_vec();
    for (end, node) in [(true, run.start_node), (false, run.end_node)] {
        if roads.street_degree(node) == 0 || points.len() < 2 {
            continue;
        }
        let back = crossing_reach_to(roads, node, &points, end, PATH_HALF_WIDTH, true);
        let (a, b) = if end { (points[1], points[0]) } else { (points[points.len() - 2], points[points.len() - 1]) };
        let len = a.distance(b);
        if len <= back + 1.0 {
            continue;
        }
        let d = (b - a) / len;
        let stop = b - d * back;
        if end {
            points[0] = stop;
        } else {
            let n = points.len();
            points[n - 1] = stop;
        }
        // A small apron onto the pavement.
        let right = Vec2::new(-d.y, d.x);
        let apron = [stop + right * (PATH_HALF_WIDTH + 1.2), stop + right * (PATH_HALF_WIDTH + 1.2) + d * 1.6, stop - right * (PATH_HALF_WIDTH + 1.2) + d * 1.6, stop - right * (PATH_HALF_WIDTH + 1.2)];
        g.part(Paving).ground_polygon(&apron, ASPHALT_Y + 0.02, rgba(mul(pal.paving, 1.05)));
    }
    let pts = &points;
    let w = PATH_HALF_WIDTH * 2.0;
    g.part(Paving).path_strip(pts, 0.0, w, ASPHALT_Y + 0.02, rgba(mul(pal.paving, 1.05)));
    g.part(Concrete).path_box(pts, PATH_HALF_WIDTH + 0.1, 0.2, 0.0, 0.08, rgba(pal.curb));
    g.part(Concrete).path_box(pts, -(PATH_HALF_WIDTH + 0.1), 0.2, 0.0, 0.08, rgba(pal.curb));
    // Centre line for bikes to keep to one side.
    g.part(Paint).path_strip(pts, 0.0, 0.08, MARK_Y, rgba(mul(pal.lane_divider, 0.6)));
    // [pt-walk] Where it climbs steeply, steps on it.
    if let Some(prof) = profile {
        stairs(g, pal, pts, |p| prof.height(p));
    }
    let len = path_length(pts);
    let line = Polyline::new(pts);
    let at = |s: f32, off: f32| {
        let (p, d) = line.at(s);
        let right = Vec2::new(-d.y, d.x);
        (p + right * off, d)
    };
    let mut s = 8.0;
    let mut side = 1.0;
    while s < len - 4.0 {
        let (p, _) = at(s, side * (PATH_HALF_WIDTH + 0.6));
        lamp_post_world(g, p, 4.0, mul(pal.lamp, 0.9));
        s += 22.0;
        side = -side;
    }
    let mut s = 14.0 + rng.random_range(0.0..10.0);
    while s < len - 6.0 {
        let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
        let (p, d) = at(s, side * (PATH_HALF_WIDTH + 0.9));
        if rng.random_bool(0.6) {
            bench_world(g, p, d.y.atan2(d.x) + if side > 0.0 { std::f32::consts::PI } else { 0.0 }, pal.wood, CHARCOAL);
        } else {
            bin_world(g, p, mul(pal.metal_paint(0), 0.9));
        }
        s += 26.0 + rng.random_range(0.0..20.0);
    }
    // Bollards where the path meets a street.
    for (s_end, node) in [(0.0, run.start_node), (len, run.end_node)] {
        if roads.street_degree(node) == 0 {
            continue;
        }
        let back = if s_end == 0.0 { 1.0 } else { -1.0 };
        let (p, d) = point_along(pts, (s_end + back * 2.0).clamp(0.0, len));
        let right = Vec2::new(-d.y, d.x);
        for off in [-0.9f32, 0.0, 0.9] {
            bollard_world(g, p + right * off, mul(pal.steel, 0.7));
        }
    }
}

/// [pt-walk] A path steeper than this is drawn as a flight of steps.
pub const STAIRS_FROM: f32 = 0.12;
/// Rise of one step of a path's stairs.
pub const STAIR_RISE: f32 = 0.17;

/// [pt-walk] Steps on the steep stretches of a path drawn along `pts`
/// (the geometry is set on the run's profile afterwards, `height` being
/// that profile): each step a flat tread at the height of its upper edge
/// and a riser at its lower, set on the sloping paving, so the flight
/// reads as stairs however the slope runs.
pub(super) fn stairs(g: &mut Geometry, pal: &Palette, pts: &[Vec2], height: impl Fn(Vec2) -> f32) {
    let col = rgba(mul(pal.concrete(0), 1.05));
    let half = PATH_HALF_WIDTH - 0.05;
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = a.distance(b);
        if len < 0.3 {
            continue;
        }
        let (ha, hb) = (height(a), height(b));
        let grade = (hb - ha).abs() / len;
        if grade < STAIRS_FROM {
            continue;
        }
        let d = (b - a) / len;
        let right = Vec2::new(-d.y, d.x);
        let steps = ((hb - ha).abs() / STAIR_RISE).round().max(1.0) as usize;
        let tread = len / steps as f32;
        for k in 0..steps {
            let (p0, p1) = (a + d * (tread * k as f32), a + d * (tread * (k + 1) as f32));
            // The top is level at the higher edge's height; the riser
            // stands at the lower edge.
            let (lo, hi) = if hb > ha { (p0, p1) } else { (p1, p0) };
            let top = height(hi);
            // Heights relative to the profile, which lifts every vertex
            // by its own height later.
            let v = |p: Vec2, abs: f32| Vec3::new(p.x, ASPHALT_Y + 0.02 + abs - height(p), p.y);
            let (l0, r0, l1, r1) = (lo + right * half, lo - right * half, hi + right * half, hi - right * half);
            let up = Vec3::Y;
            g.part(Concrete).quad_facing([v(l0, top), v(l1, top), v(r1, top), v(r0, top)], up, col);
            let back = Vec3::new(lo.x - hi.x, 0.0, lo.y - hi.y).normalize_or_zero();
            g.part(Concrete).quad_facing([v(l0, height(lo)), v(r0, height(lo)), v(r0, top), v(l0, top)], back, rgba(mul(pal.concrete(0), 0.85)));
            for (e0, e1, n) in [(l0, l1, Vec3::new(right.x, 0.0, right.y)), (r0, r1, Vec3::new(-right.x, 0.0, -right.y))] {
                g.part(Concrete).quad_facing([v(e0, height(e0)), v(e1, height(e1)), v(e1, top), v(e0, top)], n, rgba(mul(pal.concrete(0), 0.9)));
            }
        }
    }
}

#[cfg(test)]
mod pt_walk_tests {
    use super::*;

    fn node_at(roads: &RoadNetwork, p: Vec2) -> NodeId {
        NodeId(roads.nodes.iter().position(|n| n.pos.distance(p) < 0.1).unwrap() as u32)
    }

    /// [pt-walk] F43: a path carrying straight on from the end of a street
    /// starts right at the street's end (it stopped 27 m short, as if the
    /// street ran beside it); one leaving a street's side or at a slant
    /// runs to its kerb line (the pavement covers it), clear of the lanes.
    #[test]
    fn a_path_meets_a_junction_without_a_gap() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::ZERO], 1, 1);
        roads.add_polyline_kind(&[Vec2::ZERO, Vec2::new(200.0, 0.0)], 0, 0, RoadKind::Path);
        let n = node_at(&roads, Vec2::ZERO);
        let on = crossing_reach_to(&roads, n, &[Vec2::ZERO, Vec2::new(200.0, 0.0)], true, PATH_HALF_WIDTH, true);
        assert!(on < 2.0, "straight on from the street's end: {on} m");
        let mut side = RoadNetwork::default();
        side.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        side.add_polyline_kind(&[Vec2::ZERO, Vec2::new(0.0, 200.0)], 0, 0, RoadKind::Path);
        let n = node_at(&side, Vec2::ZERO);
        let hw = side.segment(side.node(n).segments[0]).half_width();
        let off = crossing_reach_to(&side, n, &[Vec2::ZERO, Vec2::new(0.0, 200.0)], true, PATH_HALF_WIDTH, true);
        assert!((off - hw).abs() < 0.01, "off the side: to the kerb line (under the pavement), {off}");
        let slant = crossing_reach_to(&side, n, &[Vec2::ZERO, Vec2::new(150.0, 100.0)], true, PATH_HALF_WIDTH, true);
        assert!(slant > hw / 0.55 && slant < (hw + SIDEWALK) / 0.55, "along the street at a slant it clears the carriageway, not the pavement: {slant}");
    }

    /// [pt-walk] F10: a steep path is drawn as steps, level treads one rise
    /// apart once set on its slope; a gentle one has none.
    #[test]
    fn a_steep_path_gets_level_steps() {
        let pal = Palette::default();
        let pts = [Vec2::ZERO, Vec2::new(10.0, 0.0)];
        let slope = |p: Vec2| p.x * 0.5;
        let mut g = Geometry::default();
        stairs(&mut g, &pal, &pts, slope);
        let m = g.get(Surface::Concrete);
        assert!(!m.positions.is_empty(), "steps drawn");
        // Set on the profile as the street generator does.
        let mut tops: Vec<i32> = m.positions.iter().map(|p| ((p[1] + slope(Vec2::new(p[0], p[2]))) / STAIR_RISE).round() as i32).collect();
        tops.sort();
        tops.dedup();
        let steps = (5.0 / STAIR_RISE).round() as i32;
        assert!(tops.len() as i32 >= steps, "a level for each step: {} of {steps}", tops.len());
        let mut flat = Geometry::default();
        stairs(&mut flat, &pal, &pts, |p| p.x * 0.05);
        assert!(flat.get(Surface::Concrete).positions.is_empty(), "a ramp, not stairs");
    }
}
