//! [interchange] The Interchange preset of the Road tool: a diamond
//! interchange laid out from one line drawn across the highway.
//!
//! The player draws the cross street across the highway (two clicks, one on
//! each side). The cross street passes under the highway at grade (the
//! highway is lifted over it on a short deck, as for any street drawn across
//! it), and each carriageway gets an off-ramp leaving it `DIAMOND_SPAN`
//! before the crossing and an on-ramp joining it as far after, both on the
//! carriageway's outer side, meeting the cross street at one junction per
//! side (`DIAMOND_TERMINAL` out from the carriageway), where the ramp
//! terminal gets signals like any four-way junction. The ramps leave the
//! carriageway tangentially (a 40 m lead beside it, the same lead as
//! `roads::ease_ramp_ends` gives a drawn ramp), swing out on an easy S and
//! come into the cross street square. Nothing is laid where it would cross
//! another road: the plan is refused with the spot instead, as the pillar
//! and bridge checks are.
//!
//! When a street already crosses the highway along the drawn line, it is
//! kept and only the ramps are added (the classic upgrade of an underpass
//! into an interchange).

use super::*;
use crate::roads::{segment_intersection, segment_intersection_closed, RoadKind, Segment, LANE_WIDTH, RAMP_LEAD, SHOULDER};

/// How far before (off-ramp) and after (on-ramp) the crossing a ramp joins
/// its carriageway: clear of the hump the highway makes over the cross
/// street, with room for the ramp to swing out.
pub const DIAMOND_SPAN: f32 = 150.0;
/// How far out from its carriageway's centre line a ramp meets the cross
/// street: the ramp terminal junction.
pub const DIAMOND_TERMINAL: f32 = 55.0;
/// The last straight run of a ramp into the cross street, so it meets it
/// square (on a street square to the highway).
const TERMINAL_RUN: f32 = 25.0;
/// The steepest crossing angle a diamond is laid at (degrees from square).
const MAX_SKEW_DEG: f32 = 40.0;

/// A diamond interchange as it would be laid: the gestures (the cross street
/// first unless one is there already, then the ramps) and where the ramps
/// meet the street.
#[derive(Clone, Debug, Default)]
pub struct InterchangePlan {
    pub gestures: Vec<Gesture>,
    /// The ramp terminals on the cross street, one per carriageway.
    pub terminals: Vec<Vec2>,
    /// Where the cross street passes under each carriageway.
    pub crossings: Vec<Vec2>,
    /// The street kept (a street along the line was there already).
    pub kept_street: bool,
}

/// Why a diamond cannot be laid here, and the spot to show.
#[derive(Clone, Debug, PartialEq)]
pub struct InterchangeRefusal {
    pub why: String,
    pub at: Vec2,
}

impl InterchangeRefusal {
    fn new(why: impl Into<String>, at: Vec2) -> Self {
        Self { why: why.into(), at }
    }
}

/// One carriageway the drawn line crosses.
struct Carriageway {
    at: Vec2,
    /// Direction of travel.
    d: Vec2,
    /// The outer side (right of travel: the median is on the left).
    out: Vec2,
    half_width: f32,
    /// Parameter of the crossing along the drawn line.
    t: f32,
}

fn bezier(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let u = 1.0 - t;
    p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
}

impl World {
    /// [interchange] Lay out a diamond interchange for a cross street drawn
    /// from `a` to `b` across the highway, with `lanes` (forward, back) on
    /// the street. Refused (with the spot) when the line does not cross the
    /// highway, crosses it too skew, runs too short for the ramps' junctions,
    /// or when a ramp would cross another road or join the highway where
    /// another road already does.
    pub fn plan_diamond(&self, a: Vec2, b: Vec2, lanes: (u8, u8)) -> Result<InterchangePlan, InterchangeRefusal> {
        let roads = &self.roads;
        let len = a.distance(b);
        if len < 1.0 {
            return Err(InterchangeRefusal::new("draw the cross street across the highway: click one side, then the other", a));
        }
        let u = (b - a) / len;
        // The carriageways the line crosses, in order along it.
        let mut cws: Vec<Carriageway> = Vec::new();
        for (sid, s) in roads.alive_segments() {
            if !s.is_highway() {
                continue;
            }
            let (p0, p1) = roads.segment_points(sid);
            // Closed: the highway's height samples are vertices, and a line
            // through one still crosses it.
            let Some((t, _)) = segment_intersection_closed(a, b, p0, p1, 1e-3) else { continue };
            let at = a.lerp(b, t);
            if cws.iter().any(|c| c.at.distance(at) < 1.0) {
                continue;
            }
            let d = if s.lanes_fwd > 0 { (p1 - p0).normalize_or_zero() } else { (p0 - p1).normalize_or_zero() };
            cws.push(Carriageway { at, d, out: Vec2::new(-d.y, d.x), half_width: s.half_width(), t });
        }
        cws.sort_by(|x, y| x.t.total_cmp(&y.t));
        let mid = a.lerp(b, 0.5);
        match cws.len() {
            0 => return Err(InterchangeRefusal::new("the line does not cross the highway: draw the cross street from one side of it to the other", mid)),
            1 | 2 => {}
            _ => return Err(InterchangeRefusal::new("the line crosses more than one highway: a diamond takes one (both carriageways)", cws[2].at)),
        }
        for c in &cws {
            let skew = c.d.dot(u).abs().asin().to_degrees();
            if skew > MAX_SKEW_DEG {
                return Err(InterchangeRefusal::new(format!("the street crosses the highway {skew:.0} degrees off square (at most {MAX_SKEW_DEG:.0}): turn it to cross more squarely"), c.at));
            }
        }
        // The cross street: one already along the line under the highway,
        // else the drawn one.
        let existing = self.street_along(a, b, &cws);
        let mut plan = InterchangePlan { kept_street: existing.is_some(), crossings: cws.iter().map(|c| c.at).collect(), ..Default::default() };
        if existing.is_none() {
            let (f, bk) = (lanes.0.max(1), lanes.1);
            if crate::roads::is_highway_spec(RoadKind::Street, f, bk) || crate::roads::is_ramp_spec(RoadKind::Street, f, bk) {
                return Err(InterchangeRefusal::new("the cross street must be a two-way street", mid));
            }
            plan.gestures.push(Gesture::Road { points: vec![a, b], lanes_fwd: f, lanes_bwd: bk, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        }
        // The terminal on the street as it will run: on the existing one,
        // else on the drawn line.
        let street_line = |p: Vec2| -> Result<Vec2, InterchangeRefusal> {
            if existing.is_none() {
                return Ok(p);
            }
            match roads.nearest_where(p, |s| s.is_street() && !s.is_motorway()) {
                Some((_, _, q)) if q.distance(p) < 10.0 => Ok(q),
                _ => Err(InterchangeRefusal::new(format!("the street under the highway ends before the ramps could meet it ({:.0} m out from the highway): extend it", DIAMOND_TERMINAL), p)),
            }
        };
        let mut ramps: Vec<(Vec<Vec2>, &'static str)> = Vec::new();
        for c in &cws {
            // The terminal: along the line, out on this carriageway's outer
            // side, DIAMOND_TERMINAL from its centre line.
            let sigma = if u.dot(c.out) >= 0.0 { 1.0 } else { -1.0 };
            let along_line = DIAMOND_TERMINAL / u.dot(c.out).abs().max(0.2);
            let t_pt = street_line(c.at + u * sigma * along_line)?;
            // The street must reach past it (or a street already there).
            let reach = (t_pt - a).dot(u);
            if existing.is_none() && (reach < 10.0 || reach > len - 10.0) {
                let short = if reach < 10.0 { a } else { b };
                return Err(InterchangeRefusal::new(
                    format!("the street is too short on this side: it must run at least {:.0} m out from the highway so the ramps can meet it", DIAMOND_TERMINAL + 10.0),
                    short,
                ));
            }
            let gap = c.half_width + LANE_WIDTH * 0.5 + SHOULDER + 1.0;
            // Off: leave DIAMOND_SPAN before the crossing; on: join as far
            // after. Both come into the terminal square to the street.
            let q_off = c.at - c.d * DIAMOND_SPAN;
            let q_on = c.at + c.d * DIAMOND_SPAN;
            for (q, off) in [(q_off, true), (q_on, false)] {
                // The carriageway must run on past the join.
                let beyond = q + c.d * if off { -30.0 } else { 30.0 };
                let on_cw = |p: Vec2| roads.nearest_where(p, Segment::is_highway).is_some_and(|(sid, _, x)| {
                    let (p0, p1) = roads.segment_points(sid);
                    x.distance(p) < 2.0 && (p1 - p0).normalize_or_zero().dot(c.d).abs() > 0.99
                });
                if !on_cw(q) || !on_cw(beyond) {
                    return Err(InterchangeRefusal::new("the highway bends or ends too close to the crossing for a ramp to join it here: move the cross street along it", q));
                }
                // Toward the crossing from the join, beside the carriageway.
                let toward = if off { c.d } else { -c.d };
                let lead = q + toward * RAMP_LEAD + c.out * gap;
                let approach = t_pt - toward * TERMINAL_RUN;
                let k = (approach - lead).dot(toward) / 3.0;
                let mut pts = vec![q, lead];
                let n = (((approach - lead).length() / 12.0).round() as usize).clamp(3, 10);
                for i in 1..n {
                    pts.push(bezier(lead, lead + toward * k, approach - toward * k, approach, i as f32 / n as f32));
                }
                pts.push(approach);
                pts.push(t_pt);
                if !off {
                    pts.reverse();
                }
                ramps.push((pts, if off { "off-ramp" } else { "on-ramp" }));
            }
            plan.terminals.push(t_pt);
        }
        // Nothing in the way: a ramp crosses no road but its own carriageway
        // join and its terminal, and no other road already joins the
        // carriageway where it does.
        let new_street: Option<(Vec2, Vec2)> = existing.is_none().then_some((a, b));
        // [sweep-visual] Roads drawn in the active project but not built
        // yet are in the way as much as built ones.
        let pending = self.pending_roads();
        for (pts, what) in &ramps {
            let (first, last) = (pts[0], pts[pts.len() - 1]);
            for w in pts.windows(2) {
                let (p, q) = (w[0], w[1]);
                for sid in roads.segments_in_box(p.min(q), p.max(q)) {
                    let s = roads.segment(sid);
                    let (p0, p1) = roads.segment_points(sid);
                    let Some((t, _)) = segment_intersection_closed(p, q, p0, p1, 1e-3) else { continue };
                    let x = p.lerp(q, t);
                    // Its own join, and its terminal on the street there.
                    if x.distance(first) < 3.0 || x.distance(last) < 3.0 {
                        continue;
                    }
                    let what_road = if s.is_highway() { "the highway".to_string() } else { self.road_name_at(x) };
                    return Err(InterchangeRefusal::new(format!("the {what} would cross {what_road}: move the interchange along the highway or clear the way"), x));
                }
                for (road, kind) in &pending {
                    for w2 in road.windows(2) {
                        let Some((t, _)) = segment_intersection_closed(p, q, w2[0], w2[1], 1e-3) else { continue };
                        let x = p.lerp(q, t);
                        if x.distance(first) < 3.0 || x.distance(last) < 3.0 {
                            continue;
                        }
                        return Err(InterchangeRefusal::new(format!("the {what} would cross a {} planned in this project: move the interchange or change the plan", match kind { RoadKind::Street => "road", RoadKind::Path => "path", RoadKind::Rail => "railway" }), x));
                    }
                }
                if let Some((s0, s1)) = new_street {
                    if let Some((t, _)) = segment_intersection(p, q, s0, s1) {
                        let x = p.lerp(q, t);
                        if x.distance(last) > 3.0 && x.distance(first) > 3.0 {
                            return Err(InterchangeRefusal::new(format!("the {what} would cross the cross street before its junction"), x));
                        }
                    }
                }
            }
            // The join on the carriageway: no other road joins there.
            let join = if *what == "off-ramp" { first } else { last };
            let crowded = roads.nodes.iter().enumerate().any(|(i, n)| {
                n.pos.distance(join) < RAMP_LEAD && roads.node(NodeId(i as u32)).segments.iter().any(|&s| roads.segment(s).alive && !roads.segment(s).is_highway())
            });
            // A planned road ending at the join counts too.
            let crowded = crowded || pending.iter().any(|(road, _)| [road[0], road[road.len() - 1]].iter().any(|e| e.distance(join) < RAMP_LEAD));
            if crowded {
                return Err(InterchangeRefusal::new(format!("another road already joins the highway where the {what} would: move the interchange along the highway"), join));
            }
        }
        for (pts, _) in ramps {
            plan.gestures.push(Gesture::Road { points: pts, lanes_fwd: 1, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        }
        Ok(plan)
    }

    /// [interchange] Add a diamond interchange (`plan_diamond`) to the active
    /// project (one is started if none is), as the Road tool's Interchange
    /// preset does. The gesture ids, in order.
    pub fn add_diamond(&mut self, a: Vec2, b: Vec2, lanes: (u8, u8)) -> Result<(InterchangePlan, Vec<GestureId>), InterchangeRefusal> {
        let plan = self.plan_diamond(a, b, lanes)?;
        let ids = plan.gestures.iter().cloned().map(|g| self.add_gesture(g)).collect();
        Ok((plan, ids))
    }

    /// A street (not the highway, not a slip road) already crossing every
    /// carriageway near where the line does and running along it: the
    /// piece under the highway of an underpass there.
    fn street_along(&self, a: Vec2, b: Vec2, cws: &[Carriageway]) -> Option<SegmentId> {
        let u = (b - a).normalize_or_zero();
        let c = cws.iter().fold(Vec2::ZERO, |acc, c| acc + c.at) / cws.len() as f32;
        let mut best: Option<(f32, SegmentId)> = None;
        for sid in self.roads.segments_in_box(c - Vec2::splat(30.0), c + Vec2::splat(30.0)) {
            let s = self.roads.segment(sid);
            if !s.is_street() || s.is_motorway() || s.lanes_fwd == 0 || s.lanes_bwd == 0 {
                continue;
            }
            let (p0, p1) = self.roads.segment_points(sid);
            let dir = (p1 - p0).normalize_or_zero();
            if dir.dot(u).abs() < 0.97 {
                continue;
            }
            let d = crate::geom::project_on_segment(c, p0, p1).1.distance(c);
            if d < 8.0 && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, sid));
            }
        }
        best.map(|(_, s)| s)
    }

    /// [sweep-visual] The roads the active project adds or changes and has
    /// not built yet, and their kind.
    fn pending_roads(&self) -> Vec<(Vec<Vec2>, RoadKind)> {
        // [w1-roads] As they will be built: a curved road's control points
        // can stand well off its line (a bend drawn across the ramp's way
        // was missed), and a planned roundabout counts as its ring.
        self.planned_ways().into_iter().map(|(pts, kind, _, _)| (pts, kind)).collect()
    }

    /// A road for a message: where it is (no street-name lookup).
    fn road_name_at(&self, p: Vec2) -> String {
        format!("a road at ({:.0}, {:.0})", p.x, p.y)
    }
}
