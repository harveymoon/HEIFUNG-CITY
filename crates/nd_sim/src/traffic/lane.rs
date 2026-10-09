//! Lanes: their kinds, geometry and toll marks, and the curves of the
//! connector lanes that join them through a junction.

use super::*;

#[derive(Clone, Debug)]
pub enum LaneKind {
    Road { segment: SegmentId, forward: bool, index: u8 },
    Connector { node: NodeId, approach: usize },
}

#[derive(Clone, Debug)]
pub struct Lane {
    pub points: Vec<Vec2>,
    pub(super) cum: Vec<f32>,
    pub length: f32,
    pub next: Vec<LaneId>,
    pub kind: LaneKind,
    /// Cars on this lane ordered by increasing position.
    pub cars: Vec<CarId>,
    pub green: bool,
    /// Limit at the lane's start, m/s. Most lanes have one limit; a ramp's
    /// lanes ease from it to `limit_end` along their length.
    pub speed_limit: f32,
    /// Limit at the lane's end (equal to `speed_limit` except on ramps).
    pub limit_end: f32,
    /// A lane of a highway carriageway: its traffic has priority at merges.
    pub highway: bool,
    /// A lane of a highway slip road or a connector along one.
    pub ramp: bool,
    pub end_node: NodeId,
    /// Parallel lanes of the same segment and direction, if any.
    pub left: Option<LaneId>,
    pub right: Option<LaneId>,
    /// Lanes that lead into this one.
    pub prev: Vec<LaneId>,
    /// For connectors: -1 a left turn, 1 a right turn, 0 straight on.
    pub turn: i8,
    /// For road lanes ending at a junction: the movements allowed from it
    /// (`LEFT | STRAIGHT | RIGHT`); 0 elsewhere.
    pub arrows: u8,
    /// For connectors: the arrows of the lane feeding it allow this turn.
    /// Routes only use permitted connectors; a car already routed keeps going.
    pub permitted: bool,
    /// Index into `LaneNetwork::approaches` for road lanes ending at a junction.
    pub approach: u32,
    /// [driving] Connectors: seconds since the light went red (0 on green),
    /// for drivers deciding whether to run a late amber.
    pub red_for: f32,
    /// [driving] Connectors closed by a street takeover: nobody enters.
    pub blocked: bool,
    /// [rail-crossing] Connectors over a level crossing that is warning or
    /// closed (set by the world every tick): drivers stop at the stop line.
    pub rail_closed: bool,
    /// [tolls] A toll booth across this lane, set by the world after every
    /// rebuild (`World::apply_tolls`).
    pub toll: Option<LaneToll>,
    /// [events] How rough the road surface is, `0..1` (worn streets,
    /// `crate::road_wear`): set by the world after every rebuild and each
    /// night. Drivers keep under the limit (`limit_at`) and misjudge more.
    pub rough: f32,
}

/// [tolls] A toll booth on a lane: where along it, what it charges and how
/// slowly vehicles pass it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneToll {
    /// `crate::tolls::TollBooth::id`.
    pub booth: u32,
    /// Distance along the lane of the booth line.
    pub s: f32,
    pub price: f32,
    /// Speed through the booth, m/s.
    pub speed: f32,
}

/// [tolls] A vehicle drove through a booth this tick; the world charges it.
#[derive(Clone, Copy, Debug)]
pub struct TollPass {
    pub car: CarId,
    pub booth: u32,
    pub price: f32,
}

impl Lane {
    pub(super) fn new(points: Vec<Vec2>, kind: LaneKind, end_node: NodeId, speed_limit: f32) -> Self {
        let mut cum = vec![0.0];
        for w in points.windows(2) {
            cum.push(cum.last().unwrap() + w[0].distance(w[1]));
        }
        let length = *cum.last().unwrap();
        Self { points, cum, length, next: Vec::new(), kind, cars: Vec::new(), green: true, speed_limit, limit_end: speed_limit, highway: false, ramp: false, end_node, left: None, right: None, prev: Vec::new(), turn: 0, arrows: 0, permitted: true, approach: u32::MAX, red_for: 0.0, blocked: false, rail_closed: false, toll: None, rough: 0.0 }
    }

    pub fn is_connector(&self) -> bool {
        matches!(self.kind, LaneKind::Connector { .. })
    }

    /// The speed limit `s` metres along the lane.
    pub fn limit_at(&self, s: f32) -> f32 {
        // [events] A worn surface is driven under the limit.
        let rough = if self.rough > 0.0 { crate::road_wear::slow_factor(self.rough) } else { 1.0 };
        if self.limit_end == self.speed_limit || self.length <= 0.0 {
            return self.speed_limit * rough;
        }
        let t = (s / self.length).clamp(0.0, 1.0);
        (self.speed_limit + (self.limit_end - self.speed_limit) * t) * rough
    }

    /// Set one limit for the whole lane.
    pub fn set_limit(&mut self, v: f32) {
        self.speed_limit = v;
        self.limit_end = v;
    }

    /// True when `other` is the lane beside this one (a lane change).
    pub fn is_beside(&self, other: LaneId) -> bool {
        self.left == Some(other) || self.right == Some(other)
    }

    /// Position and unit direction at distance `s` along the lane.
    pub fn point_at(&self, s: f32) -> (Vec2, Vec2) {
        if self.points.len() < 2 {
            return (self.points.first().copied().unwrap_or(Vec2::ZERO), Vec2::X);
        }
        let s = s.clamp(0.0, self.length);
        let mut i = 0;
        while i + 2 < self.points.len() && self.cum[i + 1] < s {
            i += 1;
        }
        let seg_len = (self.cum[i + 1] - self.cum[i]).max(1e-4);
        let t = ((s - self.cum[i]) / seg_len).clamp(0.0, 1.0);
        let dir = (self.points[i + 1] - self.points[i]).normalize_or_zero();
        (self.points[i].lerp(self.points[i + 1], t), if dir == Vec2::ZERO { Vec2::X } else { dir })
    }

    /// Closest point on the lane to `p`: `(distance along, distance from)`.
    /// Points of the lane from `s0` to `s1`, the ends interpolated.
    pub fn points_between(&self, s0: f32, s1: f32) -> Vec<Vec2> {
        let (s0, s1) = (s0.clamp(0.0, self.length), s1.clamp(0.0, self.length));
        let mut out = vec![self.point_at(s0).0];
        for (i, p) in self.points.iter().enumerate() {
            if i < self.cum.len() && self.cum[i] > s0 && self.cum[i] < s1 {
                out.push(*p);
            }
        }
        out.push(self.point_at(s1).0);
        out
    }

    pub fn project(&self, p: Vec2) -> (f32, f32) {
        let mut best = (0.0, f32::MAX);
        for (i, w) in self.points.windows(2).enumerate() {
            let (t, q) = project_on_segment(p, w[0], w[1]);
            let d = p.distance(q);
            if d < best.1 {
                best = (self.cum[i] + t * (self.cum[i + 1] - self.cum[i]), d);
            }
        }
        best
    }

    pub fn start(&self) -> Vec2 {
        self.points[0]
    }

    pub fn end(&self) -> Vec2 {
        *self.points.last().unwrap()
    }

    pub(super) fn start_dir(&self) -> Vec2 {
        (self.points[1] - self.points[0]).normalize_or_zero()
    }

    pub(super) fn end_dir(&self) -> Vec2 {
        let n = self.points.len();
        (self.points[n - 1] - self.points[n - 2]).normalize_or_zero()
    }
}

/// A cubic leaving `p0` along `d0` and arriving at `p1` along `d1`, with
/// handles `h` long: tangent at both ends, so no kink where it meets the lanes.
pub(super) fn cubic_join(p0: Vec2, d0: Vec2, p1: Vec2, d1: Vec2, h: f32, samples: usize) -> Vec<Vec2> {
    let (c0, c1) = (p0 + d0 * h, p1 - d1 * h);
    (0..=samples)
        .map(|i| {
            let t = i as f32 / samples as f32;
            let u = 1.0 - t;
            p0 * (u * u * u) + c0 * (3.0 * u * u * t) + c1 * (3.0 * u * t * t) + p1 * (t * t * t)
        })
        .collect()
}

/// Largest angle (radians) between two points of a turning connector's arc.
pub(super) const ARC_STEP: f32 = 0.14;

/// Curve joining the end of one lane to the start of another inside an
/// intersection: the path a driver takes, staying in lane.
///
/// A turn runs straight on along the arriving lane, round a circular arc
/// tangent to both lanes (the fillet of the corner where the two lane
/// lines meet, as tight as the shorter leg allows) and straight out along
/// the exit lane, sampled every 8 degrees. Lanes that do not meet inside
/// the junction (a shallow bend, a dead-end U-turn, lanes side by side)
/// get a cubic tangent to both. Straight through stays a straight line.
///
/// The corner is `p_in + d_in * a` = `p_out - d_out * b`; for a real turn
/// both `a` and `b` are positive. (The old code tested the second
/// parameter for being positive with the opposite sign, so every turn fell
/// back to the chord's midpoint as its control point: a straight diagonal
/// across the corner, which is what read as cars cutting corners.)
pub(super) fn connector_curve(p_in: Vec2, d_in: Vec2, p_out: Vec2, d_out: Vec2, radius: f32) -> Vec<Vec2> {
    let dist = p_in.distance(p_out);
    if dist < 0.5 {
        return vec![p_in, p_out];
    }
    let cos = d_in.dot(d_out).clamp(-1.0, 1.0);
    if cos > 0.98 {
        // Straight on: a line when the lanes line up, else a gentle S.
        if (p_out - p_in).normalize_or_zero().dot(d_in) > 0.999 {
            return vec![p_in, p_out];
        }
        return cubic_join(p_in, d_in, p_out, d_out, dist * 0.4, 8);
    }
    let denom = d_in.perp_dot(d_out);
    if denom.abs() > 1e-3 {
        let qp = p_out - p_in;
        let a = qp.perp_dot(d_out) / denom;
        let b = -qp.perp_dot(d_in) / denom;
        // Keep the corner inside the junction so turns never swing out over
        // the kerb, even at shallow angles.
        let limit = radius * 1.3 + 1.0;
        if a > 0.05 && b > 0.05 && a <= limit && b <= limit {
            let corner = p_in + d_in * a;
            let theta = cos.acos();
            let leg = a.min(b);
            let r = leg / (theta * 0.5).tan();
            let (t1, t2) = (corner - d_in * leg, corner + d_out * leg);
            let side = denom.signum();
            let centre = t1 + Vec2::new(-d_in.y, d_in.x) * side * r;
            let (v1, v2) = (t1 - centre, t2 - centre);
            let sweep = v1.perp_dot(v2).atan2(v1.dot(v2));
            let n = ((sweep.abs() / ARC_STEP).ceil() as usize).max(2);
            let mut pts = vec![p_in];
            for i in 0..=n {
                let ang = sweep * i as f32 / n as f32;
                let (sn, cs) = ang.sin_cos();
                let p = centre + Vec2::new(v1.x * cs - v1.y * sn, v1.x * sn + v1.y * cs);
                if pts.last().is_none_or(|q: &Vec2| q.distance(p) > 0.05) {
                    pts.push(p);
                }
            }
            if pts.last().is_none_or(|q| q.distance(p_out) > 0.05) {
                pts.push(p_out);
            } else if let Some(l) = pts.last_mut() {
                *l = p_out;
            }
            return pts;
        }
    }
    // A U-turn or a bend whose lanes meet outside the junction.
    let h = if cos < -0.9 { (dist * 0.7).max(2.0) } else { dist * 0.4 };
    cubic_join(p_in, d_in, p_out, d_out, h, 12)
}
