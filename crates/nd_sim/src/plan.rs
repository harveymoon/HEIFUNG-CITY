//! Planning: gestures (roads and zones as drawn by the player), the master
//! plan (what is built), and projects (pending changes with undo/redo).
//!
//! Everything in the world's road network, zones and lots is derived from the
//! master plan, so implementing a project is "apply deltas, rebuild".

use crate::zoning::LandUse;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GestureId(pub u64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Gesture {
    Road { points: Vec<Vec2>, lanes_fwd: u8, lanes_bwd: u8 },
    Zone { polygon: Vec<Vec2>, land_use: LandUse },
}

impl Gesture {
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
    pub fn roads(&self) -> impl Iterator<Item = (GestureId, &[Vec2], u8, u8)> {
        self.gestures.iter().filter_map(|(id, g)| match g {
            Gesture::Road { points, lanes_fwd, lanes_bwd } => Some((*id, points.as_slice(), *lanes_fwd, *lanes_bwd)),
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
    pub planned_roads: Vec<(GestureId, Vec<Vec2>, u8, u8)>,
    /// Master roads that will be removed or replaced.
    pub removed_roads: Vec<(GestureId, Vec<Vec2>, u8, u8)>,
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
            Gesture::Road { points, lanes_fwd, lanes_bwd } => out.planned_roads.push((*id, points.clone(), *lanes_fwd, *lanes_bwd)),
            Gesture::Zone { polygon, land_use } => out.planned_zones.push((*id, polygon.clone(), *land_use)),
        }
    }
    for (id, g) in &master.gestures {
        let still_same = effective.gestures.get(id) == Some(g);
        if still_same {
            continue;
        }
        match g {
            Gesture::Road { points, lanes_fwd, lanes_bwd } => out.removed_roads.push((*id, points.clone(), *lanes_fwd, *lanes_bwd)),
            Gesture::Zone { polygon, .. } => out.removed_zones.push((*id, polygon.clone())),
        }
    }
    out
}
