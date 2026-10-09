//! Runs set on their height profiles.

use super::*;

/// A run's height profile: its points with the node heights, and between
/// them the inner samples of any long street (`World::road_inner`) as
/// extra points, so the run is lifted onto the way the cars drive on.
pub(super) fn run_profile(roads: &RoadNetwork, run: &Run, node_z: &impl Fn(NodeId) -> f32, inner: &[Vec<(f32, f32)>]) -> (Vec<Vec2>, Vec<f32>) {
    let mut pts = vec![run.points[0]];
    let mut z = vec![node_z(run.nodes[0])];
    for k in 0..run.points.len() - 1 {
        let (na, nb) = (run.nodes[k], run.nodes[k + 1]);
        let seg = roads.node(na).segments.iter().copied().find(|&s| {
            let sg = roads.segment(s);
            sg.alive && ((sg.a == na && sg.b == nb) || (sg.a == nb && sg.b == na))
        });
        if let Some(samples) = seg.and_then(|s| inner.get(s.0 as usize).filter(|v| !v.is_empty()).map(|v| (roads.segment(s).a == na, v))) {
            let (forward, v) = samples;
            let (a, b) = (run.points[k], run.points[k + 1]);
            let mut add = |t: f32, h: f32| {
                pts.push(a.lerp(b, t));
                z.push(h);
            };
            if forward {
                for &(t, h) in v.iter() {
                    add(t, h);
                }
            } else {
                for &(t, h) in v.iter().rev() {
                    add(1.0 - t, h);
                }
            }
        }
        pts.push(run.points[k + 1]);
        z.push(node_z(nb));
    }
    (pts, z)
}

/// Height along a run at the point of it nearest `p`, from the heights of
/// its nodes. This scans every leg of the run, which is fine for one point
/// but not for every vertex of a long run: the street generator uses
/// [`RunProfile`], which gives the same answer from a grid of legs.
pub fn profile_height(points: &[Vec2], z: &[f32], p: Vec2) -> f32 {
    if points.len() < 2 || z.len() != points.len() {
        return z.first().copied().unwrap_or(0.0);
    }
    let mut best = (f32::INFINITY, 0.0f32);
    for (i, w) in points.windows(2).enumerate() {
        let (t, q) = nd_sim::roads::project_on_segment(p, w[0], w[1]);
        let d = q.distance_squared(p);
        if d < best.0 {
            best = (d, z[i] + (z[i + 1] - z[i]) * t);
        }
    }
    best.1
}

/// A run's height profile with its legs binned on a grid, so lifting every
/// vertex drawn for the run costs a few leg tests each instead of one per
/// leg of the run. Setting runs on their profile used to scan the whole run
/// per vertex: the 10 km highway has ~90 legs and tens of thousands of
/// vertices (barrier pieces every 2 m), and with relief on every road edit
/// stalled the frame for about a second.
///
/// Each leg is listed in every cell within `reach` of it, so for a vertex
/// within `reach` of the run its nearest leg is among its cell's candidates
/// and the answer is exactly [`profile_height`]'s (candidates are kept in
/// leg order and ties go to the earlier leg, as the scan does). Anything
/// farther out (nothing a street draws) falls back to the full scan.
pub struct RunProfile<'a> {
    pub(super) points: &'a [Vec2],
    pub(super) z: &'a [f32],
    pub(super) cell: f32,
    pub(super) reach: f32,
    pub(super) cells: std::collections::HashMap<(i32, i32), Vec<u32>>,
}

impl<'a> RunProfile<'a> {
    /// Below this many legs the plain scan is as quick as the grid.
    const SMALL: usize = 6;

    pub fn new(points: &'a [Vec2], z: &'a [f32], reach: f32) -> Self {
        let cell = 24.0f32;
        let mut cells: std::collections::HashMap<(i32, i32), Vec<u32>> = Default::default();
        if points.len() > Self::SMALL + 1 && z.len() == points.len() {
            // The farthest a cell's corner lies from its centre: a cell is
            // listed for a leg if any point of it may be within `reach`.
            let slack = reach + cell * std::f32::consts::FRAC_1_SQRT_2;
            for (i, w) in points.windows(2).enumerate() {
                let lo = w[0].min(w[1]) - Vec2::splat(reach);
                let hi = w[0].max(w[1]) + Vec2::splat(reach);
                let (x0, y0) = ((lo.x / cell).floor() as i32, (lo.y / cell).floor() as i32);
                let (x1, y1) = ((hi.x / cell).floor() as i32, (hi.y / cell).floor() as i32);
                for cx in x0..=x1 {
                    for cy in y0..=y1 {
                        let c = Vec2::new((cx as f32 + 0.5) * cell, (cy as f32 + 0.5) * cell);
                        if nd_sim::roads::project_on_segment(c, w[0], w[1]).1.distance(c) <= slack {
                            cells.entry((cx, cy)).or_default().push(i as u32);
                        }
                    }
                }
            }
        }
        Self { points, z, cell, reach, cells }
    }

    /// The height at the point of the run nearest `p`.
    pub fn height(&self, p: Vec2) -> f32 {
        if self.cells.is_empty() {
            return profile_height(self.points, self.z, p);
        }
        let key = ((p.x / self.cell).floor() as i32, (p.y / self.cell).floor() as i32);
        let mut best = (f32::INFINITY, 0.0f32);
        for &i in self.cells.get(&key).map(|v| v.as_slice()).unwrap_or(&[]) {
            let i = i as usize;
            let (t, q) = nd_sim::roads::project_on_segment(p, self.points[i], self.points[i + 1]);
            let d = q.distance_squared(p);
            if d < best.0 {
                best = (d, self.z[i] + (self.z[i + 1] - self.z[i]) * t);
            }
        }
        if best.0 <= self.reach * self.reach {
            best.1
        } else {
            profile_height(self.points, self.z, p)
        }
    }
}
