//! Railway track, level crossings and footpaths: the runs that are not streets.

use super::*;

/// Single track: ballast, sleepers, two rails, drawn by the same hand as a
/// freight yard's sidings.
pub(super) fn railway(g: &mut Geometry, pal: &Palette, roads: &RoadNetwork, run: &Run, points: &[Vec2]) {
    // Stopped at a level crossing's edge, where `level_crossing` lays the
    // rails into the road: the ballast, sleepers and rails used to run on
    // over the asphalt and the kerbs, standing 0.3 m proud of the road.
    let (t0, t1) = (crossing_reach(roads, run.start_node, points, true, RAIL_HALF_WIDTH), crossing_reach(roads, run.end_node, points, false, RAIL_HALF_WIDTH));
    let pts = if t0 == 0.0 && t1 == 0.0 { points.to_vec() } else { trim(points, t0, t1) };
    railway_track(g, pal, &pts, RAIL_HALF_WIDTH * 2.0);
}

/// How far along a path or track from its end at `node` it stays back for
/// the streets crossing there: out to the back of their pavements, measured
/// along it (further at a slant). Zero where no street meets it.
pub(super) fn crossing_reach(roads: &RoadNetwork, node: NodeId, points: &[Vec2], start: bool, half: f32) -> f32 {
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
            let along = (b - a).normalize_or_zero();
            let sin = d.perp_dot(along).abs().max(0.35);
            // Its own width's corners reach further at a slant.
            (roads.segment(s).half_width() + SIDEWALK + half * d.dot(along).abs()) / sin
        })
        .fold(0.0, f32::max)
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
pub(super) fn footpath(g: &mut Geometry, pal: &Palette, roads: &RoadNetwork, run: &Run, points: &[Vec2], rng: &mut impl Rng) {
    // Where the path meets a street, stop at the back of that street's
    // pavement and flare out onto it, instead of running under the kerb.
    let mut points = points.to_vec();
    for (end, node) in [(true, run.start_node), (false, run.end_node)] {
        if roads.street_degree(node) == 0 || points.len() < 2 {
            continue;
        }
        let back = crossing_reach(roads, node, &points, end, PATH_HALF_WIDTH);
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
