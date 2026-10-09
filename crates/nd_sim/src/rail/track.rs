//! Track geometry: smoothing a drawn line into curves trains can take.

use super::*;

/// Make a drawn line of track fit for trains: ends that join existing
/// track leave or arrive along it through a turnout, corners sharper than
/// the spline can soften are eased to the minimum radius, and the result
/// is a smooth curve sampled every few metres.
pub fn smooth_track(points: &[Vec2], roads: &RoadNetwork) -> Vec<Vec2> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let mut pts: Vec<Vec2> = points.to_vec();
    for end in [false, true] {
        let n = pts.len();
        let (i_end, i_next) = if end { (n - 1, n - 2) } else { (0, 1) };
        let p = pts[i_end];
        let Some((sid, _, q)) = roads.nearest_rail(p) else { continue };
        if q.distance(p) > END_SNAP_RADIUS {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let mut t = (b - a).normalize_or_zero();
        // Travel away from the switch towards the rest of the line.
        let away = (pts[i_next] - q).normalize_or_zero();
        if t.dot(away) < 0.0 {
            t = -t;
        }
        let side = Vec2::new(-t.y, t.x);
        let side = if side.dot(away) >= 0.0 { side } else { -side };
        let lead = q + t * SWITCH_LEAD + side * 4.0;
        pts[i_end] = q;
        if end {
            pts.insert(i_end, lead);
        } else {
            pts.insert(1, lead);
        }
    }
    // Ease sharp corners: a corner becomes two points along its legs.
    let mut eased: Vec<Vec2> = vec![pts[0]];
    for i in 1..pts.len() - 1 {
        let (a, b, c) = (pts[i - 1], pts[i], pts[i + 1]);
        let d1 = (b - a).normalize_or_zero();
        let d2 = (c - b).normalize_or_zero();
        let turn = d1.dot(d2).clamp(-1.0, 1.0).acos();
        if turn < EASE_TURN {
            eased.push(b);
            continue;
        }
        let tangent = (MIN_RADIUS * (turn * 0.5).tan()).min(a.distance(b) * 0.5).min(b.distance(c) * 0.5);
        eased.push(b - d1 * tangent);
        eased.push(b + d2 * tangent);
    }
    eased.push(*pts.last().unwrap());
    eased.dedup_by(|a, b| a.distance(*b) < 0.5);
    // Long legs get waypoints so the spline runs straight between the
    // bends instead of bulging towards the next control point.
    let mut dense: Vec<Vec2> = vec![eased[0]];
    for w in eased.windows(2) {
        let len = w[0].distance(w[1]);
        let n = (len / 30.0).floor() as usize;
        for k in 1..n {
            dense.push(w[0].lerp(w[1], k as f32 / n as f32));
        }
        dense.push(w[1]);
    }
    crate::plan::sample_curve(&dense)
}

/// The sharpest turn between consecutive legs of a polyline, in radians.
pub fn sharpest_turn(pts: &[Vec2]) -> f32 {
    let mut worst = 0.0f32;
    for w in pts.windows(3) {
        let d1 = (w[1] - w[0]).normalize_or_zero();
        let d2 = (w[2] - w[1]).normalize_or_zero();
        worst = worst.max(d1.dot(d2).clamp(-1.0, 1.0).acos());
    }
    worst
}
