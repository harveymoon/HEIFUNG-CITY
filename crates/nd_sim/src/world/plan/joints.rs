//! [fix/roads-tools] Junctions as one joint (todo 2026-10-11): the roads
//! meeting at a junction move together when the Edit tool drags it, and
//! implementing mends roads whose ends meet (or that run through a
//! junction) so they share one control point there.
//!
//! A junction is a node of the built network where roads of two or more
//! gestures meet. Its members are the roads with a control point there
//! (an end dropped on it, snapped within the network's reach) and the
//! roads running through it (a control point is put in them, on the
//! line, when the junction is dragged). Mending is geometry neutral: the
//! network laid from the mended plan must be the one laid from the plan
//! as it was, or nothing is mended.

use super::*;
use crate::roads::{END_SNAP_RADIUS, GRADE_SEPARATION, SNAP_RADIUS};
use std::collections::BTreeSet;

/// Control points of different gestures this close are one joint.
pub const JOINT_SAME: f32 = 0.5;
/// A road whose built line passes this close to a junction runs through it.
const THROUGH_REACH: f32 = 0.75;

/// [fix/roads-tools] A junction as the Edit tool moves it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Joint {
    /// Where it is (the network node, or the shared control point).
    pub at: Vec2,
    /// The built node, when the network has one there.
    pub node: Option<NodeId>,
    /// Roads with a control point at the junction: (gesture, point index).
    pub points: Vec<(GestureId, usize)>,
    /// Roads running through it: (gesture, control leg the junction is on).
    pub through: Vec<(GestureId, usize)>,
}

impl Joint {
    /// Every gesture that meets here.
    pub fn gestures(&self) -> BTreeSet<GestureId> {
        self.points.iter().chain(&self.through).map(|(g, _)| *g).collect()
    }

    /// Two or more roads (or both ends of one loop) meet here: dragging it
    /// moves them together.
    pub fn is_junction(&self) -> bool {
        self.points.len() + self.through.len() >= 2
    }

    /// The gestures with a point put in on each road running through (on
    /// its line, at the junction), and the control points that move with
    /// the junction. Moving those points of these gestures is the drag.
    pub fn handles(&self, plan: &Plan) -> (BTreeMap<GestureId, Gesture>, Vec<(GestureId, usize)>) {
        let mut out: BTreeMap<GestureId, Gesture> = BTreeMap::new();
        let mut moving: Vec<(GestureId, usize)> = Vec::new();
        for &(id, i) in &self.points {
            if let Some(g) = plan.gestures.get(&id) {
                out.entry(id).or_insert_with(|| g.clone());
                moving.push((id, i));
            }
        }
        for &(id, _) in &self.through {
            let Some(g) = plan.gestures.get(&id) else { continue };
            let g = out.entry(id).or_insert_with(|| g.clone());
            let Some((leg, _)) = leg_near(g, self.at) else { continue };
            let at = insert_on_leg(g, leg, self.at);
            // Points after the one put in move up one.
            for m in moving.iter_mut().filter(|m| m.0 == id && m.1 >= at) {
                m.1 += 1;
            }
            moving.push((id, at));
        }
        (out, moving)
    }

    /// The gestures with the junction moved to `to`: every road attached
    /// keeps meeting the others there.
    pub fn moved(&self, plan: &Plan, to: Vec2) -> Vec<(GestureId, Gesture)> {
        let (mut gs, moving) = self.handles(plan);
        for (id, i) in moving {
            if let Some(g) = gs.get_mut(&id) {
                g.move_point(i, to);
            }
        }
        gs.into_iter().collect()
    }
}

/// Put a control point at `q` on control leg `leg` of a road (after point
/// `leg`), its level blended along the leg so a raised road keeps its
/// profile. Returns the new point's index.
pub fn insert_on_leg(g: &mut Gesture, leg: usize, q: Vec2) -> usize {
    let Gesture::Road { points, elevation, .. } = g else { return 0 };
    if points.len() < 2 {
        return 0;
    }
    let leg = leg.min(points.len() - 2);
    let (a, b) = (points[leg], points[leg + 1]);
    let (t, _) = crate::geom::project_on_segment(q, a, b);
    if elevation.iter().any(|l| *l != 0.0) {
        elevation.resize(points.len(), 0.0);
        let l = elevation[leg] + (elevation[leg + 1] - elevation[leg]) * t;
        elevation.insert(leg + 1, l);
    }
    points.insert(leg + 1, q);
    leg + 1
}

/// The control leg of road `g` nearest `q`, and how far `q` is from the
/// road's built line (for a curve, the sampled spline).
pub fn leg_near(g: &Gesture, q: Vec2) -> Option<(usize, f32)> {
    let Gesture::Road { points, .. } = g else { return None };
    if points.len() < 2 {
        return None;
    }
    let d = crate::geom::distance_to_polyline(q, &g.road_polyline());
    let leg = (0..points.len() - 1).min_by(|&a, &b| {
        let da = crate::geom::project_on_segment(q, points[a], points[a + 1]).1.distance(q);
        let db = crate::geom::project_on_segment(q, points[b], points[b + 1]).1.distance(q);
        da.total_cmp(&db)
    })?;
    Some((leg, d))
}

/// Level of a road at `q` on control leg `leg`.
fn level_on_leg(g: &Gesture, leg: usize, q: Vec2) -> f32 {
    let pts = g.points();
    if leg + 1 >= pts.len() {
        return g.level(leg);
    }
    let (t, _) = crate::geom::project_on_segment(q, pts[leg], pts[leg + 1]);
    g.level(leg) + (g.level(leg + 1) - g.level(leg)) * t
}

impl World {
    /// Does node `n` carry a piece of the road along `line`: some live
    /// piece from it runs along the line (within `reach` at its middle)?
    fn node_on_road(roads: &RoadNetwork, n: NodeId, line: &[Vec2], reach: f32) -> bool {
        let node = roads.node(n);
        node.segments.iter().filter_map(|&s| roads.try_segment(s)).any(|s| {
            let other = if s.a == n { s.b } else { s.a };
            let mid = (node.pos + roads.node(other).pos) * 0.5;
            crate::geom::distance_to_polyline(mid, line) <= reach
        })
    }

    /// [fix/roads-tools] The junction at built node `n` under `plan`:
    /// every road of the plan meeting there. A roundabout's nodes have none
    /// (the ring is moved by its centre).
    pub fn joint_of_node(&self, plan: &Plan, n: NodeId) -> Joint {
        Self::joint_in(&self.roads, plan, n)
    }

    /// [`World::joint_of_node`] on network `roads` (one laid from `plan`).
    pub(in crate::world) fn joint_in(roads: &RoadNetwork, plan: &Plan, n: NodeId) -> Joint {
        let node = roads.node(n);
        let (q, level) = (node.pos, node.level);
        let mut j = Joint { at: q, node: Some(n), ..Default::default() };
        if roads.ring_of_node(n).is_some() {
            return j;
        }
        for (id, g) in &plan.gestures {
            let Gesture::Road { points, curved, .. } = g else { continue };
            if points.len() < 2 {
                continue;
            }
            let (lo, hi) = crate::geom::bounds(points);
            let pad = if *curved { 30.0 } else { END_SNAP_RADIUS + 1.0 };
            if q.x < lo.x - pad || q.y < lo.y - pad || q.x > hi.x + pad || q.y > hi.y + pad {
                continue;
            }
            let last = points.len() - 1;
            // The nearest control point at the junction.
            let near = (0..points.len())
                .map(|i| (i, points[i].distance(q)))
                .filter(|&(i, d)| d <= if i == 0 || i == last { END_SNAP_RADIUS } else { SNAP_RADIUS })
                .filter(|&(i, _)| (g.level(i) - level).abs() < GRADE_SEPARATION)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            let line = g.road_polyline();
            if let Some((i, d)) = near {
                if Self::node_on_road(roads, n, &line, (d * 0.6).max(1.0)) {
                    j.points.push((*id, i));
                    // Both ends of a closed loop meet here.
                    if (i == 0 || i == last) && points[0].distance(points[last]) <= JOINT_SAME {
                        j.points.push((*id, if i == 0 { last } else { 0 }));
                    }
                    continue;
                }
            }
            if let Some((leg, d)) = leg_near(g, q) {
                // [w1-roads] Or bent onto it: a road crossing a curve snaps
                // onto the curve's nearest vertex (up to `SNAP_RADIUS`
                // off its own line), and its pieces there run along it.
                let on = (d <= THROUGH_REACH && Self::node_on_road(roads, n, &line, 1.0)) || (d <= SNAP_RADIUS && Self::node_on_road(roads, n, &line, (d * 0.6).max(1.0)));
                if on && (level_on_leg(g, leg, q) - level).abs() < GRADE_SEPARATION {
                    j.through.push((*id, leg));
                }
            }
        }
        j
    }

    /// [fix/roads-tools] The junction the Edit tool would grab at `p`
    /// (within `pick` metres): a built junction node where two or more of
    /// the plan's roads meet, else the nearest road control point with any
    /// others exactly on it. None when no road point is that near.
    pub fn joint_at(&self, plan: &Plan, p: Vec2, pick: f32) -> Option<Joint> {
        // The nearest road control point.
        let mut best: Option<(f32, GestureId, usize, Vec2)> = None;
        for (id, g) in &plan.gestures {
            if !matches!(g, Gesture::Road { .. }) {
                continue;
            }
            for (i, &c) in g.points().iter().enumerate() {
                let d = c.distance(p);
                if d < pick && best.is_none_or(|b| d < b.0) {
                    best = Some((d, *id, i, c));
                }
            }
        }
        // A built junction at that point (or, with none, under the pointer).
        let probe = best.map_or(p, |b| b.3);
        let reach = if best.is_some() { SNAP_RADIUS } else { pick };
        let node = self.roads.node_near(probe, reach).filter(|&n| self.roads.node(n).segments.iter().filter(|&&s| self.roads.try_segment(s).is_some()).count() >= 2);
        if let Some(n) = node {
            let j = self.joint_of_node(plan, n);
            let has_best = best.is_none_or(|b| j.points.contains(&(b.1, b.2)) || j.through.iter().any(|t| t.0 == b.1));
            if j.is_junction() && j.gestures().len() >= 2 && has_best {
                return Some(j);
            }
        }
        let (_, id, i, c) = best?;
        let mut j = Joint { at: c, node: None, points: vec![(id, i)], through: Vec::new() };
        let level = plan.gestures[&id].level(i);
        for (oid, g) in &plan.gestures {
            if !matches!(g, Gesture::Road { .. }) {
                continue;
            }
            for (k, &q) in g.points().iter().enumerate() {
                if (*oid, k) != (id, i) && q.distance(c) <= JOINT_SAME && (g.level(k) - level).abs() < GRADE_SEPARATION {
                    j.points.push((*oid, k));
                }
            }
        }
        Some(j)
    }

    /// [fix/roads-tools] Mend the master plan's junctions: road ends the
    /// network snapped onto a junction are put exactly on it, and straight
    /// roads running through one get a control point there, so the Edit
    /// tool's junction has one point to move. Only kept when the network
    /// laid from the mended plan is exactly the one built (nothing on the
    /// map changes). How many gestures changed.
    pub(in crate::world) fn mend_junctions(&mut self) -> usize {
        let changes = self.junction_mends(&self.master);
        if changes.is_empty() {
            return 0;
        }
        let mut mended = self.master.clone();
        for (id, g) in &changes {
            mended.gestures.insert(*id, g.clone());
        }
        let (roads, _) = self.lay_roads(&mended);
        if !same_geometry(&self.roads, &roads) {
            return 0;
        }
        let n = changes.len();
        self.master = mended;
        self.plan_changed();
        n
    }

    /// [w1-roads] Curved roads running through a junction get a control
    /// point there too, before the plan is built: done after the build
    /// it would move the curve (a spline bends round a new point) and so
    /// was left undone, and the junction stayed a point on the curve the
    /// Edit tool could pull the roads apart at. Laid from the plan about to
    /// be built, so the curve is built once, through its new point: the
    /// point goes in where the network meets it, with points of the drawn
    /// curve pinned beside it until the curve built through them strays
    /// no more than `CURVE_MEND_TOL` from the curve drawn (else it is left
    /// as drawn). Only junctions of the roads in `touched` (the project's).
    /// How many roads were mended.
    pub(in crate::world) fn mend_curves_through(&mut self, touched: &BTreeSet<GestureId>) -> usize {
        let curved = |g: &Gesture| matches!(g, Gesture::Road { curved: true, .. });
        if touched.is_empty() || !self.master.gestures.values().any(curved) {
            return 0;
        }
        let (roads, _) = self.lay_roads(&self.master);
        let mut out: BTreeMap<GestureId, Gesture> = BTreeMap::new();
        let mut done: Vec<(Vec2, BTreeSet<GestureId>)> = Vec::new();
        for i in 0..roads.nodes.len() {
            let n = NodeId(i as u32);
            let live = roads.node(n).segments.iter().filter(|&&s| roads.try_segment(s).is_some()).count();
            if live < 3 {
                continue;
            }
            let j = Self::joint_in(&roads, &self.master, n);
            let gs = j.gestures();
            if gs.len() < 2 || !gs.iter().any(|g| touched.contains(g)) {
                continue;
            }
            // One crossing can leave two junction nodes a few metres apart
            // (a road crossing a curve between two of its vertices): it is
            // mended once, at the first.
            if done.iter().any(|(p, o): &(Vec2, BTreeSet<GestureId>)| p.distance(j.at) < 2.0 * SNAP_RADIUS && o.intersection(&gs).count() >= 2) {
                continue;
            }
            // The curves running through get their point; then every other
            // road here is put on the same point too (ends moved onto it,
            // straight roads through bent onto it, as the network already
            // bends them), so the junction is laid at that one point
            // however the curve is sampled round it.
            let mut mended_curve = false;
            for &(id, _) in &j.through {
                let Some(g) = out.get(&id).or_else(|| self.master.gestures.get(&id)) else { continue };
                if !curved(g) {
                    continue;
                }
                if let Some(m) = curve_through(g, j.at) {
                    out.insert(id, m);
                    mended_curve = true;
                }
            }
            if !mended_curve {
                continue;
            }
            done.push((j.at, gs.clone()));
            for &(id, k) in &j.points {
                let Some(g) = out.get(&id).or_else(|| self.master.gestures.get(&id)).cloned() else { continue };
                let mut g = g;
                if g.points().get(k).is_some_and(|c| *c != j.at) {
                    g.move_point(k, j.at);
                    out.insert(id, g);
                }
            }
            for &(id, _) in &j.through {
                let Some(g) = out.get(&id).or_else(|| self.master.gestures.get(&id)).cloned() else { continue };
                if curved(&g) {
                    continue;
                }
                let mut g = g;
                if let Some((leg, d)) = leg_near(&g, j.at) {
                    let pts = g.points();
                    if d <= SNAP_RADIUS && pts[leg].distance(j.at) > JOINT_SAME && pts[leg + 1].distance(j.at) > JOINT_SAME {
                        insert_on_leg(&mut g, leg, j.at);
                        out.insert(id, g);
                    }
                }
            }
        }
        let n = out.len();
        for (id, g) in out {
            self.master.gestures.insert(id, g);
        }
        n
    }

    /// The gestures of `plan` with their junctions mended (see
    /// `mend_junctions`), only those that change.
    pub(in crate::world) fn junction_mends(&self, plan: &Plan) -> Vec<(GestureId, Gesture)> {
        let mut out: BTreeMap<GestureId, Gesture> = BTreeMap::new();
        for (i, node) in self.roads.nodes.iter().enumerate() {
            let live = node.segments.iter().filter(|&&s| self.roads.try_segment(s).is_some()).count();
            if live < 2 {
                continue;
            }
            let j = self.joint_of_node(plan, NodeId(i as u32));
            if j.gestures().len() < 2 {
                continue;
            }
            for &(id, k) in &j.points {
                let g = out.entry(id).or_insert_with(|| plan.gestures[&id].clone());
                if g.points().get(k).is_some_and(|c| *c != j.at) {
                    g.move_point(k, j.at);
                }
            }
            for &(id, _) in &j.through {
                // A curve would bend round a new point: it is left as drawn
                // (the Edit tool puts the point in when the junction moves).
                if matches!(plan.gestures.get(&id), Some(Gesture::Road { curved: true, .. })) {
                    continue;
                }
                let g = out.entry(id).or_insert_with(|| plan.gestures[&id].clone());
                // The leg as it is now: an earlier junction may have put a
                // point in this road already.
                if let Some((leg, d)) = leg_near(g, j.at) {
                    let pts = g.points();
                    if d <= SNAP_RADIUS && pts[leg].distance(j.at) > JOINT_SAME && pts[leg + 1].distance(j.at) > JOINT_SAME {
                        insert_on_leg(g, leg, j.at);
                    }
                }
            }
        }
        out.into_iter().filter(|(id, g)| plan.gestures.get(id) != Some(g)).collect()
    }
}

/// [w1-roads] How far a curve mended through a junction may stray from
/// the curve drawn, metres.
pub const CURVE_MEND_TOL: f32 = 0.2;

/// Furthest any point of one polyline is from the other, both ways.
fn polyline_gap(a: &[Vec2], b: &[Vec2]) -> f32 {
    let one = |x: &[Vec2], y: &[Vec2]| x.iter().map(|p| crate::geom::distance_to_polyline(*p, y)).fold(0.0f32, f32::max);
    one(a, b).max(one(b, a))
}

/// [w1-roads] Curved road `g` with a control point at `q` (a junction on
/// its built line) that runs within `CURVE_MEND_TOL` of where it ran. A
/// spline bends round a new point over the spans either side of it, so
/// those spans are pinned with points of the curve as drawn, every
/// `spacing` metres (40, then 25, then 15) until the curve built through
/// them keeps to the one drawn. None when it has a point there already,
/// is a closed loop, or cannot be kept close enough.
pub fn curve_through(g: &Gesture, q: Vec2) -> Option<Gesture> {
    let Gesture::Road { points, curved: true, lanes_fwd, lanes_bwd, kind, elevation, car_only, .. } = g else { return None };
    if points.len() < 2 || crate::plan::is_loop(points) || points.iter().any(|c| c.distance(q) <= JOINT_SAME) {
        return None;
    }
    let drawn = g.road_polyline();
    let drawn_levels = g.road_levels();
    let (s_q, d_q) = crate::geom::project_on_poly(&drawn, q)?;
    if d_q > THROUGH_REACH {
        return None;
    }
    // Arc length of every control point along the curve drawn (it passes
    // through each of them).
    let arcs: Vec<f32> = points.iter().map(|c| crate::geom::project_on_poly(&drawn, *c).map_or(0.0, |(s, _)| s)).collect();
    if arcs.windows(2).any(|w| w[1] < w[0]) {
        return None;
    }
    let leg = arcs.windows(2).position(|w| s_q >= w[0] && s_q <= w[1])?;
    let lo = arcs[leg.saturating_sub(1)];
    let hi = arcs[(leg + 2).min(points.len() - 1)];
    let raised = elevation.iter().any(|l| *l != 0.0);
    // Level at arc `s` along the curve drawn.
    let level_at = |s: f32| -> f32 {
        let mut acc = 0.0;
        for i in 0..drawn.len().saturating_sub(1) {
            let len = drawn[i].distance(drawn[i + 1]);
            if s <= acc + len || i + 2 == drawn.len() {
                let t = if len > 0.0 { ((s - acc) / len).clamp(0.0, 1.0) } else { 0.0 };
                let (a, b) = (drawn_levels.get(i).copied().unwrap_or(0.0), drawn_levels.get(i + 1).copied().unwrap_or(0.0));
                return a + (b - a) * t;
            }
            acc += len;
        }
        0.0
    };
    for spacing in [40.0f32, 25.0, 15.0] {
        let mut pts: Vec<(f32, Vec2, f32)> = points.iter().zip(&arcs).enumerate().map(|(i, (p, s))| (*s, *p, elevation.get(i).copied().unwrap_or(0.0))).collect();
        pts.push((s_q, q, level_at(s_q)));
        let mut s = lo + spacing;
        while s < hi - spacing * 0.4 {
            if pts.iter().all(|(t, ..)| (t - s).abs() > spacing * 0.4) {
                pts.push((s, crate::geom::point_along(&drawn, s).0, level_at(s)));
            }
            s += spacing;
        }
        pts.sort_by(|a, b| a.0.total_cmp(&b.0));
        let m = Gesture::Road {
            points: pts.iter().map(|p| p.1).collect(),
            lanes_fwd: *lanes_fwd,
            lanes_bwd: *lanes_bwd,
            curved: true,
            kind: *kind,
            elevation: if raised { pts.iter().map(|p| p.2).collect() } else { Vec::new() },
            car_only: *car_only,
        };
        if polyline_gap(&m.road_polyline(), &drawn) <= CURVE_MEND_TOL {
            return Some(m);
        }
    }
    None
}

/// [fix/roads-tools] Do two networks lay the same roads: the same pieces
/// between the same points at the same levels, with the same lanes, kind
/// and classification, the same rings? Unlike `same_layout`, the order the
/// nodes and pieces were numbered in does not count (a road with a point
/// put in at a junction it already met lays its nodes in another order).
pub(in crate::world) fn same_geometry(a: &RoadNetwork, b: &RoadNetwork) -> bool {
    type Key = ([u32; 6], u8, u8, u8);
    fn pieces(n: &RoadNetwork) -> Vec<Key> {
        let mut v: Vec<Key> = n
            .alive_segments()
            .map(|(_, s)| {
                let (na, nb) = (n.node(s.a), n.node(s.b));
                let ka = [na.pos.x.to_bits(), na.pos.y.to_bits(), na.level.to_bits()];
                let kb = [nb.pos.x.to_bits(), nb.pos.y.to_bits(), nb.level.to_bits()];
                let flags = (s.kind as u8) | (u8::from(s.ring) << 3) | (u8::from(s.ramp) << 4);
                if ka <= kb {
                    ([ka[0], ka[1], ka[2], kb[0], kb[1], kb[2]], s.lanes_fwd, s.lanes_bwd, flags)
                } else {
                    ([kb[0], kb[1], kb[2], ka[0], ka[1], ka[2]], s.lanes_bwd, s.lanes_fwd, flags)
                }
            })
            .collect();
        v.sort_unstable();
        v
    }
    a.rings == b.rings && pieces(a) == pieces(b)
}
