//! Planning: gestures (roads, zones and painted parcels as drawn by the
//! player), the master plan (what is built), and projects (named sets of
//! pending changes, each with its own undo/redo).
//!
//! Everything in the world's road network, zones and lots is derived from the
//! master plan, so implementing a project is "apply deltas, rebuild".
//!
//! Parcels are cut along every road whether or not anyone zoned them
//! ([`crate::zoning::generate_parcels`]); zoning only gives them a land use.
//! [`parcel_zoning`] folds the plan's zone gestures over the parcels, oldest
//! first, so the newest gesture that covers a parcel decides it.

use crate::roads::RoadKind;
use crate::zoning::{LandUse, ParcelKey, ParcelMap};
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
    /// Parcels painted with the zone brush or marquee: each parcel named by
    /// its stable key gets `land_use`, or goes back to unzoned with `None`.
    /// One brush stroke is one gesture. Industrial strokes may also set the
    /// industry sub-zone of the painted ground (`industry`: an
    /// `IndustryKind` key, or "auto" to let the ground decide), applied when
    /// the project is implemented, so it is undone with the stroke.
    ParcelZone {
        parcels: Vec<ParcelKey>,
        land_use: Option<LandUse>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        industry: Option<String>,
    },
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
            Gesture::ParcelZone { .. } => Vec::new(),
        }
    }

    /// Control points: a road's or a zone's corners. Painted parcels have none.
    pub fn points(&self) -> &[Vec2] {
        match self {
            Gesture::Road { points, .. } => points,
            Gesture::Zone { polygon, .. } => polygon,
            Gesture::ParcelZone { .. } => &[],
        }
    }

    /// The control points to edit, if the gesture has any.
    pub fn points_mut(&mut self) -> Option<&mut Vec<Vec2>> {
        match self {
            Gesture::Road { points, .. } => Some(points),
            Gesture::Zone { polygon, .. } => Some(polygon),
            Gesture::ParcelZone { .. } => None,
        }
    }

    pub fn is_road(&self) -> bool {
        matches!(self, Gesture::Road { .. })
    }

    /// Minimum number of points for the gesture to be valid.
    pub fn min_points(&self) -> usize {
        match self {
            Gesture::Road { .. } => 2,
            Gesture::Zone { .. } => 3,
            Gesture::ParcelZone { .. } => 0,
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

/// A named set of pending changes. Several can be planned side by side;
/// one is active (edited, and what the tools see as the plan), the others
/// are previewed dimmed and implemented one at a time.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Project {
    /// Stable id (0 until the world numbers it: a save from before named
    /// projects gets one on load).
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub name: String,
    pub steps: Vec<PlanDelta>,
    pub redo: Vec<PlanDelta>,
    /// Bumped on every change so previews know when to rebuild.
    pub version: u64,
}

impl Project {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Record several gestures as one step (one undo).
    pub fn push_many(&mut self, changes: Vec<(GestureId, Option<Gesture>)>) {
        if changes.is_empty() {
            return;
        }
        self.steps.push(changes.into_iter().collect());
        self.redo.clear();
        self.version += 1;
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

    /// Gestures both projects change: implemented one after the other, the
    /// later one's version wins.
    pub fn conflicts_with(&self, other: &Project) -> Vec<GestureId> {
        let mine: std::collections::BTreeSet<GestureId> = self.touched().collect();
        let theirs: std::collections::BTreeSet<GestureId> = other.touched().collect();
        mine.intersection(&theirs).copied().collect()
    }

    /// Parcel keys this project paints (with a land use, or back to unzoned).
    pub fn painted(&self) -> std::collections::BTreeSet<ParcelKey> {
        let mut out = std::collections::BTreeSet::new();
        for step in &self.steps {
            for g in step.values().flatten() {
                if let Gesture::ParcelZone { parcels, .. } = g {
                    out.extend(parcels.iter().copied());
                }
            }
        }
        out
    }
}

/// Land use and deciding gesture of every parcel, by parcel index.
pub type ParcelZoning = Vec<Option<(LandUse, GestureId)>>;

/// What each parcel is zoned as under `plan`. Gestures are applied oldest
/// first (gesture ids only grow), so the newest zone polygon that holds the
/// parcel's centre, or the newest stroke that painted it, decides. A parcel
/// takes a land use only if it accepts it ([`crate::zoning::Parcel::accepts`]);
/// a gesture it refuses leaves it as it was. A stroke with no land use
/// erases back to unzoned.
pub fn parcel_zoning(parcels: &ParcelMap, plan: &Plan) -> ParcelZoning {
    let mut out: Vec<Option<(LandUse, GestureId)>> = vec![None; parcels.len()];
    for (id, g) in &plan.gestures {
        match g {
            Gesture::Zone { polygon, land_use } => {
                if polygon.len() < 3 || crate::geom::self_intersects(polygon) {
                    continue;
                }
                for i in parcels.in_polygon(polygon) {
                    if parcels.parcels[i].accepts(*land_use) {
                        out[i] = Some((*land_use, *id));
                    }
                }
            }
            Gesture::ParcelZone { parcels: keys, land_use, .. } => {
                for k in keys {
                    let Some(i) = parcels.find(k) else { continue };
                    match land_use {
                        Some(lu) if parcels.parcels[i].accepts(*lu) => out[i] = Some((*lu, *id)),
                        Some(_) => {}
                        None => out[i] = None,
                    }
                }
            }
            Gesture::Road { .. } => {}
        }
    }
    out
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
    /// Paint strokes added, changed or taken back (the parcels they colour
    /// are in `World::planned_zoning`).
    pub parcel_strokes: usize,
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
            Gesture::ParcelZone { .. } => out.parcel_strokes += 1,
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
            Gesture::ParcelZone { .. } => out.parcel_strokes += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn road(x: f32) -> Gesture {
        Gesture::Road { points: vec![Vec2::new(x, 0.0), Vec2::new(x, 100.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street }
    }

    #[test]
    fn projects_undo_redo_and_amend() {
        let mut master = Plan::default();
        master.gestures.insert(GestureId(1), road(0.0));
        let mut p = Project::default();
        assert!(p.is_empty() && !p.undo() && !p.redo());
        p.push(GestureId(2), Some(road(10.0)));
        p.push(GestureId(1), None);
        assert_eq!(p.version, 2);
        let eff = p.apply_to(&master);
        assert_eq!(eff.gestures.keys().copied().collect::<Vec<_>>(), vec![GestureId(2)], "the new road is in, the master road is removed");
        assert!(p.undo());
        assert_eq!(p.apply_to(&master).gestures.len(), 2, "undo brings the master road back");
        assert!(p.redo());
        assert_eq!(p.apply_to(&master).gestures.len(), 1);
        // A new step after an undo clears the redo stack.
        assert!(p.undo());
        p.push(GestureId(3), Some(road(20.0)));
        assert!(!p.redo());
        assert_eq!(p.apply_to(&master).gestures.len(), 3);
        // Amending replaces the last step when it is the same gesture...
        let steps = p.steps.len();
        p.amend(GestureId(3), Some(road(25.0)));
        assert_eq!(p.steps.len(), steps);
        assert_eq!(p.apply_to(&master).gestures[&GestureId(3)].points()[0].x, 25.0);
        // ...and records a new one otherwise.
        p.amend(GestureId(2), Some(road(11.0)));
        assert_eq!(p.steps.len(), steps + 1);
        let touched: Vec<GestureId> = p.touched().collect();
        assert!(touched.contains(&GestureId(2)) && touched.contains(&GestureId(3)));
        // Undoing an amended step drops the whole drag.
        assert!(p.undo() && p.undo());
        assert!(!p.apply_to(&master).gestures.contains_key(&GestureId(3)));
        assert_eq!(p.apply_to(&master).gestures[&GestureId(2)].points()[0].x, 10.0);
    }

    #[test]
    fn preview_reports_planned_and_removed_gestures() {
        let mut master = Plan::default();
        master.gestures.insert(GestureId(1), road(0.0));
        master.gestures.insert(GestureId(2), Gesture::Zone { polygon: vec![Vec2::ZERO, Vec2::X, Vec2::Y], land_use: LandUse::Park });
        let mut p = Project::default();
        p.push(GestureId(1), Some(road(5.0)));
        p.push(GestureId(2), None);
        p.push(GestureId(3), Some(road(50.0)));
        let pv = preview(&master, &p);
        assert_eq!(pv.planned_roads.iter().map(|r| r.0).collect::<Vec<_>>(), vec![GestureId(1), GestureId(3)]);
        assert_eq!(pv.removed_roads.iter().map(|r| r.0).collect::<Vec<_>>(), vec![GestureId(1)], "a moved road is removed and planned");
        assert_eq!(pv.removed_zones.len(), 1);
        assert!(pv.planned_zones.is_empty());
        assert_eq!(pv.control_points.len(), 4, "two points per planned road");
        assert!(preview(&master, &Project::default()).planned_roads.is_empty());
    }

    #[test]
    fn curves_pass_through_control_points_and_survive_duplicates() {
        let pts = [Vec2::ZERO, Vec2::new(50.0, 40.0), Vec2::new(100.0, 0.0), Vec2::new(150.0, 40.0)];
        let curve = sample_curve(&pts);
        assert_eq!(curve[0], pts[0]);
        assert!(curve.last().unwrap().distance(pts[3]) < 1e-3);
        for p in &pts {
            assert!(curve.iter().any(|q| q.distance(*p) < 1e-3), "passes through {p:?}");
        }
        assert!(curve.windows(2).all(|w| w[0].distance(w[1]) <= CURVE_STEP * 1.5), "sampled about every CURVE_STEP");
        // Fewer than three points: the polyline itself.
        assert_eq!(sample_curve(&pts[..2]), pts[..2].to_vec());
        // A doubled control point (the player clicked twice) gives no NaN
        // and still ends where it should.
        let doubled = [Vec2::ZERO, Vec2::new(50.0, 40.0), Vec2::new(50.0, 40.0), Vec2::new(100.0, 0.0)];
        let curve = sample_curve(&doubled);
        assert!(curve.iter().all(|p| p.is_finite()));
        assert!(curve.last().unwrap().distance(doubled[3]) < 1e-3);
        assert!(curve.iter().any(|q| q.distance(doubled[1]) < 1e-3));
        // Collinear control points stay on their line, in order.
        let line = [Vec2::ZERO, Vec2::new(40.0, 0.0), Vec2::new(70.0, 0.0), Vec2::new(150.0, 0.0)];
        let curve = sample_curve(&line);
        assert!(curve.iter().all(|p| p.y.abs() < 1e-3), "no bulge off a straight line");
        assert!(curve.windows(2).all(|w| w[1].x >= w[0].x - 1e-3), "monotonic along the line");
        assert!(curve.last().unwrap().distance(line[3]) < 1e-3);
        // The Edit tool's handles sit on the built road.
        let g = Gesture::Road { points: pts.to_vec(), lanes_fwd: 1, lanes_bwd: 1, curved: true, kind: RoadKind::Street };
        assert!(g.road_polyline().len() > pts.len());
        assert_eq!(g.min_points(), 2);
        assert_eq!(Gesture::Zone { polygon: Vec::new(), land_use: LandUse::Park }.min_points(), 3);
    }
}
