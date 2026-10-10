//! Planning: gestures (roads, zones and painted parcels as drawn by the
//! player), the master plan (what is built), and projects (named sets of
//! pending changes, each with its own undo/redo).
//!
//! Everything in the world's road network, zones and lots is derived from the
//! master plan, so implementing a project is "apply deltas, rebuild".
//!
//! Parcels are cut along every road whether or not anyone zoned them
//! ([`crate::zoning::generate_parcels_with`]); zoning only gives them a land use.
//! [`parcel_zoning`] folds the plan's zone gestures over the parcels, oldest
//! first, so the newest gesture that covers a parcel decides it.

use crate::roads::{RoadKind, Ring};
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
        /// Height above the ground at each control point, metres (positive
        /// on a viaduct, negative in a cutting); missing points are at
        /// grade, so roads from before elevation load as they were.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        elevation: Vec<f32>,
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
    /// A roundabout stamped over the roads: a one-way ring of `lanes`
    /// lanes, `radius` from the centre to the middle of the ring road.
    /// Built, it cuts the streets and paths older than it out of the
    /// circle and joins their ends to the ring; newer roads are kept inside
    /// and out and join it where they meet it (`roads::layout_rings`). The
    /// inside stays open land. The road gestures themselves are untouched,
    /// so undoing the stamp restores them.
    Roundabout { center: Vec2, radius: f32, lanes: u8 },
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

/// Levels along [`sample_curve`]`(pts)`: each sample takes the level of its
/// span's ends, blended by how far along the span it is (the spline passes
/// through every control point, so control points keep theirs).
pub fn sample_curve_levels(pts: &[Vec2], levels: &[f32]) -> Vec<f32> {
    let lv = |i: usize| levels.get(i).copied().unwrap_or(0.0);
    if pts.len() < 3 {
        return (0..pts.len()).map(lv).collect();
    }
    let mut out = vec![lv(0)];
    for i in 0..pts.len() - 1 {
        let steps = ((pts[i].distance(pts[i + 1]) / CURVE_STEP).ceil() as usize).max(1);
        for k in 1..=steps {
            let f = k as f32 / steps as f32;
            out.push(lv(i) + (lv(i + 1) - lv(i)) * f);
        }
    }
    out
}

/// Levels for `dst`, a polyline laid along `src` (split at the water, cut
/// back by a roundabout, eased into track curves): each point takes the
/// level of the nearest place on `src`, searched forward from the last
/// match so a road crossing itself keeps its order.
pub fn carry_levels(src: &[Vec2], levels: &[f32], dst: &[Vec2]) -> Vec<f32> {
    if levels.iter().all(|l| *l == 0.0) || src.len() < 2 {
        return vec![levels.first().copied().unwrap_or(0.0); dst.len()];
    }
    let lv = |i: usize| levels.get(i).copied().unwrap_or(0.0);
    let mut cursor = 0usize;
    dst.iter()
        .map(|&p| {
            let mut best = (f32::INFINITY, cursor, 0.0f32);
            for j in cursor..src.len() - 1 {
                let (t, q) = crate::geom::project_on_segment(p, src[j], src[j + 1]);
                let d = q.distance(p);
                if d + 1e-3 < best.0 {
                    best = (d, j, t);
                }
            }
            cursor = best.1;
            lv(best.1) + (lv(best.1 + 1) - lv(best.1)) * best.2
        })
        .collect()
}

impl Gesture {
    /// Level of control point `i` of a road (0 for anything else).
    pub fn level(&self, i: usize) -> f32 {
        match self {
            Gesture::Road { elevation, .. } => elevation.get(i).copied().unwrap_or(0.0),
            _ => 0.0,
        }
    }

    /// Does any control point leave the ground?
    pub fn is_elevated(&self) -> bool {
        matches!(self, Gesture::Road { elevation, .. } if elevation.iter().any(|l| *l != 0.0))
    }

    /// Levels of the built polyline, one per point of `road_polyline`.
    pub fn road_levels(&self) -> Vec<f32> {
        match self {
            Gesture::Road { points, curved: true, elevation, .. } => sample_curve_levels(points, elevation),
            Gesture::Road { points, elevation, .. } => (0..points.len()).map(|i| elevation.get(i).copied().unwrap_or(0.0)).collect(),
            _ => vec![0.0; self.road_polyline().len()],
        }
    }

    /// Insert a control point (a road's level there is blended from its
    /// neighbours), keeping the levels in step with the points.
    pub fn insert_point(&mut self, idx: usize, p: Vec2) {
        if let Gesture::Road { points, elevation, .. } = self {
            if elevation.iter().any(|l| *l != 0.0) {
                elevation.resize(points.len(), 0.0);
                let a = if idx > 0 { elevation.get(idx - 1).copied() } else { None };
                let b = elevation.get(idx).copied();
                let l = match (a, b) {
                    (Some(a), Some(b)) => (a + b) * 0.5,
                    (Some(x), None) | (None, Some(x)) => x,
                    (None, None) => 0.0,
                };
                elevation.insert(idx.min(elevation.len()), l);
            }
        }
        if let Some(pts) = self.points_mut() {
            let i = idx.min(pts.len());
            pts.insert(i, p);
        }
    }

    /// Remove a control point and its level.
    pub fn remove_point(&mut self, idx: usize) {
        if let Gesture::Road { elevation, .. } = self {
            if idx < elevation.len() {
                elevation.remove(idx);
            }
        }
        if let Some(pts) = self.points_mut() {
            if idx < pts.len() {
                pts.remove(idx);
            }
        }
    }


    /// The polyline a road is actually built from: the control points, or the
    /// sampled spline for curved roads.
    pub fn road_polyline(&self) -> Vec<Vec2> {
        match self {
            Gesture::Road { points, curved: true, .. } => sample_curve(points),
            Gesture::Road { points, .. } => points.clone(),
            Gesture::Zone { polygon, .. } => polygon.clone(),
            Gesture::ParcelZone { .. } => Vec::new(),
            Gesture::Roundabout { center, radius, lanes } => Ring::new(*center, *radius, *lanes).polyline(&[]).0,
        }
    }

    /// Control points: a road's or a zone's corners, a roundabout's centre.
    /// Painted parcels have none.
    pub fn points(&self) -> &[Vec2] {
        match self {
            Gesture::Road { points, .. } => points,
            Gesture::Zone { polygon, .. } => polygon,
            Gesture::ParcelZone { .. } => &[],
            Gesture::Roundabout { center, .. } => std::slice::from_ref(center),
        }
    }

    /// The control points to edit, if the gesture has a list of them (a
    /// roundabout's one centre is moved with [`Gesture::move_point`]).
    pub fn points_mut(&mut self) -> Option<&mut Vec<Vec2>> {
        match self {
            Gesture::Road { points, .. } => Some(points),
            Gesture::Zone { polygon, .. } => Some(polygon),
            Gesture::ParcelZone { .. } | Gesture::Roundabout { .. } => None,
        }
    }

    /// Move control point `idx` to `p`. False if there is no such point.
    pub fn move_point(&mut self, idx: usize, p: Vec2) -> bool {
        if let Gesture::Roundabout { center, .. } = self {
            if idx == 0 {
                *center = p;
                return true;
            }
            return false;
        }
        match self.points_mut().and_then(|pts| pts.get_mut(idx)) {
            Some(q) => {
                *q = p;
                true
            }
            None => false,
        }
    }

    pub fn is_road(&self) -> bool {
        matches!(self, Gesture::Road { .. } | Gesture::Roundabout { .. })
    }

    /// The roundabout, within limits, if this is one.
    pub fn ring(&self) -> Option<Ring> {
        match self {
            Gesture::Roundabout { center, radius, lanes } => Some(Ring::new(*center, *radius, *lanes)),
            _ => None,
        }
    }

    /// Minimum number of points for the gesture to be valid.
    pub fn min_points(&self) -> usize {
        match self {
            Gesture::Road { .. } => 2,
            Gesture::Zone { .. } => 3,
            Gesture::ParcelZone { .. } => 0,
            Gesture::Roundabout { .. } => 1,
        }
    }
}

/// What a roundabout does to the roads around it, for previews: the ring
/// road, the parts of roads it takes up and where the remaining roads join
/// it, laid over `plan` (the ring itself need not be in it).
#[derive(Clone, Debug, Default)]
pub struct RingEffect {
    pub ring: Option<Ring>,
    /// The ring road, closed, in the direction of travel.
    pub road: Vec<Vec2>,
    /// Road (and path) parts inside the circle that the works remove.
    pub cut: Vec<Vec<Vec2>>,
    /// Width of each cut road, metres.
    pub cut_widths: Vec<f32>,
    /// Where arms join the ring.
    pub joins: Vec<Vec2>,
    /// Gesture ids of the roads it cuts.
    pub cut_roads: Vec<GestureId>,
    /// Another roundabout already takes this ground.
    pub blocked: bool,
}

/// May a ring cut this road and join it? Streets (not highways, which pass
/// over) and paths; never the railway.
pub fn ring_cuts(kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8) -> bool {
    match kind {
        RoadKind::Street => !crate::roads::is_highway_spec(kind, lanes_fwd, lanes_bwd),
        RoadKind::Path => true,
        RoadKind::Rail => false,
    }
}

/// The effect of stamping `ring` onto `plan`, laid out exactly as the
/// rebuild will lay it (other roundabouts in the plan come first; one this
/// ring would overlap blocks it).
pub fn ring_effect(plan: &Plan, ring: Ring) -> RingEffect {
    let blocked = plan.roundabouts().any(|(_, r)| r.overlaps(&ring) && r != ring);
    if blocked {
        return RingEffect { ring: Some(ring), road: ring.polyline(&[]).0, blocked: true, ..Default::default() };
    }
    // Already in the plan, it cuts the roads older than it; a ring about
    // to be stamped is the newest gesture of all.
    let age = plan.roundabouts().find(|(_, r)| *r == ring).map_or(u64::MAX, |(id, _)| id.0);
    let rings = [(ring, age)];
    let roads: Vec<(GestureId, Vec<Vec2>, bool, f32)> = plan.roads().map(|(id, pts, f, b, k)| (id, pts, ring_cuts(k, f, b), crate::roads::half_width_spec(k, f, b) * 2.0)).collect();
    let raised = |id: &GestureId| plan.gestures.get(id).is_some_and(Gesture::is_elevated);
    let input: Vec<(Vec<Vec2>, bool, u64)> = roads.iter().map(|(id, p, c, _)| (p.clone(), *c && near_ring(p, &ring) && !raised(id), id.0)).collect();
    let layout = crate::roads::layout_rings(&rings, &input);
    let mut out = RingEffect { ring: Some(ring), ..Default::default() };
    if let Some((_, poly, joins)) = layout.rings.iter().find(|(r, ..)| *r == ring) {
        out.road = poly.clone();
        out.joins = joins.clone();
    }
    for (i, pts) in layout.cut {
        out.cut.push(pts);
        out.cut_widths.push(roads[i].3);
        if !out.cut_roads.contains(&roads[i].0) {
            out.cut_roads.push(roads[i].0);
        }
    }
    out
}

/// Could a road come inside this ring at all (its box reaches the circle)?
fn near_ring(pts: &[Vec2], ring: &Ring) -> bool {
    let (lo, hi) = crate::geom::bounds(pts);
    let q = ring.center.clamp(lo, hi);
    q.distance(ring.center) < ring.radius
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

    /// Every roundabout, oldest first, within limits.
    pub fn roundabouts(&self) -> impl Iterator<Item = (GestureId, Ring)> + '_ {
        self.gestures.iter().filter_map(|(id, g)| g.ring().map(|r| (*id, r)))
    }

    /// The roads as built: every road polyline with the parts inside a
    /// roundabout cut away, its ends there joined to the ring, then the
    /// rings themselves (closed one-way polylines). The rings come first so
    /// the arms snap onto their vertices; a ring overlapping an older one
    /// is not built.
    pub fn built_roads(&self) -> (Vec<(GestureId, Vec<Vec2>, u8, u8, RoadKind)>, Vec<Ring>) {
        let (roads, rings) = self.built_roads_levels();
        (roads.into_iter().map(|(id, pts, _, f, b, k)| (id, pts, f, b, k)).collect(), rings)
    }

    /// [`Plan::built_roads`] with the level of every built point.
    #[allow(clippy::type_complexity)]
    pub fn built_roads_levels(&self) -> (Vec<(GestureId, Vec<Vec2>, Vec<f32>, u8, u8, RoadKind)>, Vec<Ring>) {
        let levels: BTreeMap<GestureId, Vec<f32>> = self.gestures.iter().filter(|(_, g)| g.is_elevated()).map(|(id, g)| (*id, g.road_levels())).collect();
        let level_of = |id: GestureId, src: &[Vec2], pts: &[Vec2]| match levels.get(&id) {
            Some(l) if src.len() == pts.len() && src == pts => l.clone(),
            Some(l) => carry_levels(src, l, pts),
            None => vec![0.0; pts.len()],
        };
        let (roads, rings) = self.built_roads_flat();
        let sources: BTreeMap<GestureId, Vec<Vec2>> = self.roads().map(|(id, pts, ..)| (id, pts)).collect();
        let out = roads
            .into_iter()
            .map(|(id, pts, f, b, k)| {
                let l = match sources.get(&id) {
                    Some(src) => level_of(id, src, &pts),
                    None => vec![0.0; pts.len()],
                };
                (id, pts, l, f, b, k)
            })
            .collect();
        (out, rings)
    }

    fn built_roads_flat(&self) -> (Vec<(GestureId, Vec<Vec2>, u8, u8, RoadKind)>, Vec<Ring>) {
        let rings: Vec<(Ring, u64)> = self.roundabouts().map(|(id, r)| (r, id.0)).collect();
        let roads: Vec<(GestureId, Vec<Vec2>, u8, u8, RoadKind)> = self.roads().collect();
        if rings.is_empty() {
            return (roads, Vec::new());
        }
        // A road drawn above or below the ground passes the ring by.
        let raised = |id: &GestureId| self.gestures.get(id).is_some_and(Gesture::is_elevated);
        let input: Vec<(Vec<Vec2>, bool, u64)> = roads.iter().map(|(id, p, f, b, k)| (p.clone(), ring_cuts(*k, *f, *b) && !raised(id), id.0)).collect();
        let layout = crate::roads::layout_rings(&rings, &input);
        let ids: Vec<GestureId> = self.roundabouts().filter(|(_, r)| layout.rings.iter().any(|(x, ..)| x == r)).map(|(id, _)| id).collect();
        let mut out: Vec<(GestureId, Vec<Vec2>, u8, u8, RoadKind)> = Vec::new();
        for ((ring, poly, _), id) in layout.rings.iter().zip(ids) {
            out.push((id, poly.clone(), ring.lanes, 0, RoadKind::Street));
        }
        for (i, pts) in layout.kept {
            let (id, _, f, b, k) = &roads[i];
            out.push((*id, pts, *f, *b, *k));
        }
        (out, layout.rings.iter().map(|(r, ..)| *r).collect())
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
            Gesture::Road { .. } | Gesture::Roundabout { .. } => {}
        }
    }
    out
}

/// What the renderer needs to draw a project on top of the built city.
#[derive(Clone, Debug, Default)]
pub struct PlanPreview {
    /// Roads that will exist after implementing and differ from master.
    pub planned_roads: Vec<(GestureId, Vec<Vec2>, u8, u8, RoadKind)>,
    /// The level of every point of each planned road, in step with
    /// `planned_roads`.
    pub planned_levels: Vec<Vec<f32>>,
    /// Master roads that will be removed or replaced.
    pub removed_roads: Vec<(GestureId, Vec<Vec2>, u8, u8, RoadKind)>,
    pub planned_zones: Vec<(GestureId, Vec<Vec2>, LandUse)>,
    pub removed_zones: Vec<(GestureId, Vec<Vec2>)>,
    /// Paint strokes added, changed or taken back (the parcels they colour
    /// are in `World::planned_zoning`).
    pub parcel_strokes: usize,
    /// Every control point of every road or zone in the effective plan.
    pub control_points: Vec<(GestureId, usize, Vec2)>,
    /// Roundabouts added or changed, with what they do to the roads.
    pub planned_rings: Vec<(GestureId, RingEffect)>,
    /// Built roundabouts that will be removed or replaced.
    pub removed_rings: Vec<(GestureId, Ring)>,
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
            Gesture::Road { lanes_fwd, lanes_bwd, kind, .. } => {
                out.planned_roads.push((*id, g.road_polyline(), *lanes_fwd, *lanes_bwd, *kind));
                out.planned_levels.push(g.road_levels());
            }
            Gesture::Zone { polygon, land_use } => out.planned_zones.push((*id, polygon.clone(), *land_use)),
            Gesture::ParcelZone { .. } => out.parcel_strokes += 1,
            Gesture::Roundabout { .. } => {
                if let Some(r) = g.ring() {
                    out.planned_rings.push((*id, ring_effect(&effective, r)));
                }
            }
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
            Gesture::Roundabout { .. } => out.removed_rings.extend(g.ring().map(|r| (*id, r))),
        }
    }
    out
}

/// [pt-ux] Pieces shorter than this left by [`Gesture::cut_out`] go too.
pub const CUT_MIN_PIECE: f32 = 1.0;

/// How far along `pts` (metres) the point nearest `p` lies.
pub fn arc_of(pts: &[Vec2], p: Vec2) -> f32 {
    let mut best = (f32::INFINITY, 0.0f32);
    let mut run = 0.0f32;
    for w in pts.windows(2) {
        let len = w[0].distance(w[1]);
        let (t, q) = crate::geom::project_on_segment(p, w[0], w[1]);
        let d = q.distance(p);
        if d < best.0 {
            best = (d, run + t * len);
        }
        run += len;
    }
    best.1
}

/// The point and level `s` metres along `pts`.
fn at_arc(pts: &[Vec2], levels: &[f32], s: f32) -> (Vec2, f32) {
    let lv = |i: usize| levels.get(i).copied().unwrap_or(0.0);
    let mut run = 0.0f32;
    for (i, w) in pts.windows(2).enumerate() {
        let len = w[0].distance(w[1]);
        if run + len >= s && len > 0.0 {
            let t = ((s - run) / len).clamp(0.0, 1.0);
            return (w[0].lerp(w[1], t), lv(i) + (lv(i + 1) - lv(i)) * t);
        }
        run += len;
    }
    (pts.last().copied().unwrap_or(Vec2::ZERO), lv(pts.len().saturating_sub(1)))
}

/// [pt-ux] The stretch of `pts` from `s0` to `s1` metres along it.
pub fn polyline_between(pts: &[Vec2], s0: f32, s1: f32) -> Vec<Vec2> {
    let (s0, s1) = (s0.min(s1), s0.max(s1));
    let mut out = vec![at_arc(pts, &[], s0).0];
    let mut run = 0.0f32;
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            run += pts[i - 1].distance(*p);
        }
        if run > s0 && run < s1 {
            out.push(*p);
        }
    }
    out.push(at_arc(pts, &[], s1).0);
    out
}

/// [pt-ux] `pts` (with a level per point) with the stretch from `s0` to
/// `s1` metres along it taken out: what is left before and after, each
/// with its levels; pieces under [`CUT_MIN_PIECE`] are dropped.
pub fn cut_polyline(pts: &[Vec2], levels: &[f32], s0: f32, s1: f32) -> Vec<(Vec<Vec2>, Vec<f32>)> {
    let (s0, s1) = (s0.min(s1), s0.max(s1));
    let lv = |i: usize| levels.get(i).copied().unwrap_or(0.0);
    let total: f32 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
    let mut arcs = Vec::with_capacity(pts.len());
    let mut run = 0.0f32;
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            run += pts[i - 1].distance(*p);
        }
        arcs.push(run);
    }
    let mut out = Vec::new();
    if s0 >= CUT_MIN_PIECE {
        let mut a: Vec<Vec2> = Vec::new();
        let mut al = Vec::new();
        for i in 0..pts.len() {
            if arcs[i] < s0 - 0.05 {
                a.push(pts[i]);
                al.push(lv(i));
            }
        }
        let (q, l) = at_arc(pts, levels, s0);
        a.push(q);
        al.push(l);
        if a.len() >= 2 {
            out.push((a, al));
        }
    }
    if total - s1 >= CUT_MIN_PIECE {
        let (q, l) = at_arc(pts, levels, s1);
        let mut b = vec![q];
        let mut bl = vec![l];
        for i in 0..pts.len() {
            if arcs[i] > s1 + 0.05 {
                b.push(pts[i]);
                bl.push(lv(i));
            }
        }
        if b.len() >= 2 {
            out.push((b, bl));
        }
    }
    out
}

impl Gesture {
    /// [pt-ux] A road with the stretch `s0`..`s1` metres along its built
    /// polyline ([`Gesture::road_polyline`]) taken out: the roads left
    /// (none, one or two), same kind, lanes and levels. A curved road's
    /// pieces keep its exact built shape (the sampled spline, straight
    /// legs), so the ground along what is left is cut the same way. A
    /// roundabout or a zone has no pieces: nothing is left.
    pub fn cut_out(&self, s0: f32, s1: f32) -> Vec<Gesture> {
        let Gesture::Road { lanes_fwd, lanes_bwd, kind, .. } = self else { return Vec::new() };
        let raised = self.is_elevated();
        cut_polyline(&self.road_polyline(), &self.road_levels(), s0, s1)
            .into_iter()
            .map(|(points, levels)| Gesture::Road { points, lanes_fwd: *lanes_fwd, lanes_bwd: *lanes_bwd, curved: false, kind: *kind, elevation: if raised { levels } else { Vec::new() } })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn road(x: f32) -> Gesture {
        Gesture::Road { points: vec![Vec2::new(x, 0.0), Vec2::new(x, 100.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() }
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
        let g = Gesture::Road { points: pts.to_vec(), lanes_fwd: 1, lanes_bwd: 1, curved: true, kind: RoadKind::Street, elevation: Vec::new() };
        assert!(g.road_polyline().len() > pts.len());
        assert_eq!(g.min_points(), 2);
        assert_eq!(Gesture::Zone { polygon: Vec::new(), land_use: LandUse::Park }.min_points(), 3);
    }

    fn assert_close(a: &[Vec2], b: &[Vec2]) {
        assert!(a.len() == b.len() && a.iter().zip(b).all(|(p, q)| p.distance(*q) < 1e-3), "{a:?} != {b:?}");
    }

    #[test]
    fn cut_out_leaves_the_road_either_side() {
        let g = Gesture::Road { points: vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(200.0, 0.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() };
        // The middle piece out: two roads left, ending at the cut.
        let left = g.cut_out(60.0, 140.0);
        assert_eq!(left.len(), 2);
        assert_close(left[0].points(), &[Vec2::new(0.0, 0.0), Vec2::new(60.0, 0.0)]);
        assert_close(left[1].points(), &[Vec2::new(140.0, 0.0), Vec2::new(200.0, 0.0)]);
        // An end piece: one road, keeping the control point before the cut.
        let left = g.cut_out(150.0, 200.0);
        assert_eq!(left.len(), 1);
        assert_close(left[0].points(), &[Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(150.0, 0.0)]);
        // The whole road: nothing.
        assert!(g.cut_out(0.0, 200.0).is_empty());
        // The order of the ends does not matter; a cut on a control point keeps it once.
        let left = g.cut_out(140.0, 100.0);
        assert_close(left[0].points(), &[Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)]);
        assert_close(left[1].points(), &[Vec2::new(140.0, 0.0), Vec2::new(200.0, 0.0)]);
    }

    #[test]
    fn cut_out_keeps_levels_and_the_curve_shape() {
        let g = Gesture::Road { points: vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)], lanes_fwd: 2, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: vec![0.0, 10.0] };
        let left = g.cut_out(40.0, 60.0);
        assert_eq!(left.len(), 2);
        assert!((left[0].level(1) - 4.0).abs() < 1e-3 && (left[1].level(0) - 6.0).abs() < 1e-3);
        assert!(matches!(left[1], Gesture::Road { lanes_fwd: 2, lanes_bwd: 0, .. }));
        let curve = Gesture::Road { points: vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 40.0), Vec2::new(200.0, 0.0)], lanes_fwd: 1, lanes_bwd: 1, curved: true, kind: RoadKind::Street, elevation: Vec::new() };
        let built = curve.road_polyline();
        let total: f32 = built.windows(2).map(|w| w[0].distance(w[1])).sum();
        let left = curve.cut_out(total * 0.4, total * 0.6);
        // Every point kept lies on the built curve.
        for g in &left {
            for p in g.points() {
                assert!(crate::geom::distance_to_polyline(*p, &built) < 0.01);
            }
        }
        assert!((arc_of(&built, built[3]) - built[..4].windows(2).map(|w| w[0].distance(w[1])).sum::<f32>()).abs() < 1e-2);
    }
}
