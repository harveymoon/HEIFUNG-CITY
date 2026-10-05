//! Planning: gestures (roads and zones as drawn by the player), the master
//! plan (what is built), and projects (pending changes with undo/redo).
//!
//! Everything in the world's road network, zones and lots is derived from the
//! master plan, so implementing a project is "apply deltas, rebuild".

use crate::roads::RoadKind;
use crate::zoning::LandUse;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GestureId(pub u64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Gesture {
    Road {
        points: Vec<Vec2>,
        lanes_fwd: u8,
        lanes_bwd: u8,
        /// Smooth spline through the control points instead of straight legs.
        #[serde(default)]
        curved: bool,
        /// Street with car lanes, or a path for walkers and bikes.
        #[serde(default)]
        kind: RoadKind,
    },
    Zone { polygon: Vec<Vec2>, land_use: LandUse },
}

/// Spacing of the straight pieces a curved road is built from, in metres.
pub const CURVE_STEP: f32 = 7.0;

/// Centripetal Catmull-Rom spline through `pts`, sampled every `CURVE_STEP`.
/// The road passes through every control point, so the Edit tool's handles
/// sit on the road itself.
pub fn sample_curve(pts: &[Vec2]) -> Vec<Vec2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let n = pts.len();
    let mut out = vec![pts[0]];
    for i in 0..n - 1 {
        let p0 = if i == 0 { pts[0] * 2.0 - pts[1] } else { pts[i - 1] };
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = if i + 2 < n { pts[i + 2] } else { pts[n - 1] * 2.0 - pts[n - 2] };
        let steps = ((p1.distance(p2) / CURVE_STEP).ceil() as usize).max(1);
        // Centripetal parameterisation avoids loops and cusps.
        let t0 = 0.0;
        let t1 = t0 + p0.distance(p1).sqrt().max(1e-3);
        let t2 = t1 + p1.distance(p2).sqrt().max(1e-3);
        let t3 = t2 + p2.distance(p3).sqrt().max(1e-3);
        for k in 1..=steps {
            let t = t1 + (t2 - t1) * k as f32 / steps as f32;
            let a1 = p0 * ((t1 - t) / (t1 - t0)) + p1 * ((t - t0) / (t1 - t0));
            let a2 = p1 * ((t2 - t) / (t2 - t1)) + p2 * ((t - t1) / (t2 - t1));
            let a3 = p2 * ((t3 - t) / (t3 - t2)) + p3 * ((t - t2) / (t3 - t2));
            let b1 = a1 * ((t2 - t) / (t2 - t0)) + a2 * ((t - t0) / (t2 - t0));
            let b2 = a2 * ((t3 - t) / (t3 - t1)) + a3 * ((t - t1) / (t3 - t1));
            let c = b1 * ((t2 - t) / (t2 - t1)) + b2 * ((t - t1) / (t2 - t1));
            out.push(c);
        }
    }
    out
}

impl Gesture {
    /// The polyline a road is actually built from: the control points, or the
    /// sampled spline for curved roads.
    pub fn road_polyline(&self) -> Vec<Vec2> {
        match self {
            Gesture::Road { points, curved: true, .. } => sample_curve(points),
            Gesture::Road { points, .. } => points.clone(),
            Gesture::Zone { polygon, .. } => polygon.clone(),
        }
    }

    pub fn points(&self) -> &[Vec2] {
        match self {
            Gesture::Road { points, .. } => points,
            Gesture::Zone { polygon, .. } => polygon,
        }
    }

    pub fn points_mut(&mut self) -> &mut Vec<Vec2> {
        match self {
            Gesture::Road { points, .. } => points,
            Gesture::Zone { polygon, .. } => polygon,
        }
    }

    pub fn is_road(&self) -> bool {
        matches!(self, Gesture::Road { .. })
    }

    /// Minimum number of points for the gesture to be valid.
    pub fn min_points(&self) -> usize {
        if self.is_road() {
            2
        } else {
            3
        }
    }
}

/// A set of gestures. The master plan is one of these; so is the effective
/// plan of a project (master with the project's deltas applied).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub gestures: BTreeMap<GestureId, Gesture>,
}

impl Plan {
    /// Built polylines of every road (curves already sampled).
    pub fn roads(&self) -> impl Iterator<Item = (GestureId, Vec<Vec2>, u8, u8, RoadKind)> + '_ {
        self.gestures.iter().filter_map(|(id, g)| match g {
            Gesture::Road { lanes_fwd, lanes_bwd, kind, .. } => Some((*id, g.road_polyline(), *lanes_fwd, *lanes_bwd, *kind)),
            _ => None,
        })
    }

    pub fn zones(&self) -> impl Iterator<Item = (GestureId, &[Vec2], LandUse)> {
        self.gestures.iter().filter_map(|(id, g)| match g {
            Gesture::Zone { polygon, land_use } => Some((*id, polygon.as_slice(), *land_use)),
            _ => None,
        })
    }
}

/// One undoable step: gestures set to a new version, or removed (`None`).
pub type PlanDelta = BTreeMap<GestureId, Option<Gesture>>;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Project {
    pub steps: Vec<PlanDelta>,
    pub redo: Vec<PlanDelta>,
    /// Bumped on every change so previews know when to rebuild.
    pub version: u64,
}

impl Project {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Record a new step.
    pub fn push(&mut self, id: GestureId, gesture: Option<Gesture>) {
        let mut d = BTreeMap::new();
        d.insert(id, gesture);
        self.steps.push(d);
        self.redo.clear();
        self.version += 1;
    }

    /// Replace the last step if it touches the same gesture (used while a
    /// control point is being dragged), otherwise record a new one.
    pub fn amend(&mut self, id: GestureId, gesture: Option<Gesture>) {
        if let Some(last) = self.steps.last_mut() {
            if last.len() == 1 && last.contains_key(&id) {
                last.insert(id, gesture);
                self.version += 1;
                return;
            }
        }
        self.push(id, gesture);
    }

    pub fn undo(&mut self) -> bool {
        match self.steps.pop() {
            Some(step) => {
                self.redo.push(step);
                self.version += 1;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(step) => {
                self.steps.push(step);
                self.version += 1;
                true
            }
            None => false,
        }
    }

    pub fn apply_to(&self, master: &Plan) -> Plan {
        let mut plan = master.clone();
        for step in &self.steps {
            for (id, g) in step {
                match g {
                    Some(g) => {
                        plan.gestures.insert(*id, g.clone());
                    }
                    None => {
                        plan.gestures.remove(id);
                    }
                }
            }
        }
        plan
    }

    /// Ids this project touches.
    pub fn touched(&self) -> impl Iterator<Item = GestureId> + '_ {
        self.steps.iter().flat_map(|s| s.keys().copied())
    }
}

/// What the renderer needs to draw a project on top of the built city.
#[derive(Clone, Debug, Default)]
pub struct PlanPreview {
    /// Roads that will exist after implementing and differ from master.
    pub planned_roads: Vec<(GestureId, Vec<Vec2>, u8, u8, RoadKind)>,
    /// Master roads that will be removed or replaced.
    pub removed_roads: Vec<(GestureId, Vec<Vec2>, u8, u8, RoadKind)>,
    pub planned_zones: Vec<(GestureId, Vec<Vec2>, LandUse)>,
    pub removed_zones: Vec<(GestureId, Vec<Vec2>)>,
    /// Every control point of every road or zone in the effective plan.
    pub control_points: Vec<(GestureId, usize, Vec2)>,
}

pub fn preview(master: &Plan, project: &Project) -> PlanPreview {
    let effective = project.apply_to(master);
    let mut out = PlanPreview::default();
    for (id, g) in &effective.gestures {
        for (i, p) in g.points().iter().enumerate() {
            out.control_points.push((*id, i, *p));
        }
        let changed = master.gestures.get(id) != Some(g);
        if !changed {
            continue;
        }
        match g {
            Gesture::Road { lanes_fwd, lanes_bwd, kind, .. } => out.planned_roads.push((*id, g.road_polyline(), *lanes_fwd, *lanes_bwd, *kind)),
            Gesture::Zone { polygon, land_use } => out.planned_zones.push((*id, polygon.clone(), *land_use)),
        }
    }
    for (id, g) in &master.gestures {
        let still_same = effective.gestures.get(id) == Some(g);
        if still_same {
            continue;
        }
        match g {
            Gesture::Road { lanes_fwd, lanes_bwd, kind, .. } => out.removed_roads.push((*id, g.road_polyline(), *lanes_fwd, *lanes_bwd, *kind)),
            Gesture::Zone { polygon, .. } => out.removed_zones.push((*id, polygon.clone())),
        }
    }
    out
}
