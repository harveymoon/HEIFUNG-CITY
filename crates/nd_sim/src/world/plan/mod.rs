//! Planning: projects and gestures, undo/redo, the preview, and
//! `rebuild_from_master`, which turns the master plan into roads, parcels,
//! zones, lots and buildings (eminent domain, lot matching, demolition).
//!
//! Several named projects can be pending at once. The active one lives in
//! `World::project` (so "take the project, add a gesture, implement, put it
//! back" still works for scripts); the others wait in `parked_projects`,
//! are previewed dimmed, and are implemented one at a time, each over the
//! master as the ones before it left it.

use super::*;
use crate::plan::{parcel_zoning, ParcelZoning};

/// [tools] What implementing a project would refuse on (`World::project_check`).
#[derive(Clone, Debug, Default)]
pub struct ProjectCheck {
    /// The city's own structures its roads would take.
    pub blockers: Vec<BuildingId>,
    /// The same, each with the planned road that takes it and where.
    pub blocks: Vec<Blocker>,
    /// Stretches of raised road with no room for a column.
    pub pillar_gaps: Vec<crate::pillars::Gap>,
    /// The first of them in words ("Not implemented: ...").
    pub pillar_refusal: Option<String>,
    /// What implementing would say, word for word, if it refuses: the
    /// structures taken first, then the columns. `try_implement` refuses
    /// exactly when this is set, so a plan shown clear always implements.
    pub refusal: Option<String>,
    /// Roads the project lays.
    pub roads: usize,
}

/// [tools] One of the city's own structures a project would take: which,
/// by which planned road, and where (for the red marks and "show").
#[derive(Clone, Debug)]
pub struct Blocker {
    pub building: BuildingId,
    pub name: String,
    /// Where it stands (its lot's centre): the red pin.
    pub at: Vec2,
    /// The planned road (its gesture) that takes it, if one does.
    pub road: Option<GestureId>,
    /// That road in words: "the new road off Pylon Drive (from (x, y))".
    pub road_label: String,
    /// The stretch of that road over the structure's ground (for a
    /// structure the rebuild loses, the road's nearest point, twice).
    pub a: Vec2,
    pub b: Vec2,
    /// The road runs over it; else rebuilding the lots for the project
    /// leaves it no lot (no street left to front). The label of a road the
    /// project takes out reads "taking out <street>".
    pub runs_over: bool,
}

impl Blocker {
    /// Why it refuses, in words.
    pub fn reason(&self) -> String {
        if self.runs_over {
            format!("{} would run over {} at ({:.0}, {:.0})", self.road_label, self.name, self.at.x, self.at.y)
        } else {
            format!("{} would leave {} at ({:.0}, {:.0}) with no street to front, and it would be lost", self.road_label, self.name, self.at.x, self.at.y)
        }
    }
}
use crate::zoning::{inner_center, join_parcels, make_lot, Parcel, ParcelKey, ParcelMap};

mod projects;
mod parcels;
pub(in crate::world) use parcels::refresh_lot_roads;
mod project_check;
mod rebuild;
mod removal;

/// One pending project as the UI and the remote API list it.
#[derive(Clone, Debug)]
pub struct ProjectInfo {
    pub id: u64,
    pub name: String,
    pub active: bool,
    pub steps: usize,
    pub redo: usize,
    /// Gestures another project also changes, with that project's id.
    pub conflicts: Vec<(GestureId, u64)>,
    /// Parcels another project also paints.
    pub shared_parcels: usize,
}

/// An inactive project drawn as a dim hologram: its roads and zones, and
/// the parcels whose land use it would change.
#[derive(Clone, Debug)]
pub struct ProjectPreview {
    pub id: u64,
    pub name: String,
    pub preview: PlanPreview,
    pub zoning: Vec<(usize, Option<LandUse>)>,
}

impl World {
    // ── parcels and painting ─────────────────────────────────────────────

    /// Every change to master or project goes through here: the version
    /// moves and the effective plan is rebuilt once, so the tools can read
    /// it as often as they like.
    pub(super) fn plan_changed(&mut self) {
        self.plan_version += 1;
        let plan = match &self.project {
            Some(p) => p.apply_to(&self.master),
            None => self.master.clone(),
        };
        self.plan_cache = Some((self.plan_version, plan));
        self.refresh_planned_zoning();
    }
}

/// Do two road networks have the same layout: the same nodes where they
/// were, the same pieces between them with the same lanes, kind and
/// classification, the same rings? Only what the plan decides is compared,
/// not what the world derives afterwards (bridges, decks, heights), so a
/// network rebuilt from an unchanged plan compares equal to the one built.
fn same_layout(a: &RoadNetwork, b: &RoadNetwork) -> bool {
    a.nodes.len() == b.nodes.len()
        && a.segments.len() == b.segments.len()
        && a.rings == b.rings
        && a.nodes.iter().zip(&b.nodes).all(|(x, y)| x.pos == y.pos && x.level == y.level && x.segments == y.segments)
        && a.segments.iter().zip(&b.segments).all(|(x, y)| x.a == y.a && x.b == y.b && x.lanes_fwd == y.lanes_fwd && x.lanes_bwd == y.lanes_bwd && x.alive == y.alive && x.kind == y.kind && x.ring == y.ring && x.ramp == y.ramp)
}

/// Lots bucketed by the cells their bounding boxes cover, so "which new
/// lots could contain or sit within a few metres of this point" is a look
/// at a handful of cells rather than every lot.
struct LotGrid {
    cell: f32,
    cells: HashMap<(i32, i32), Vec<LotId>>,
}

impl LotGrid {
    fn new(lots: &SlotMap<LotId, Lot>) -> Self {
        let cell = 64.0;
        let mut cells: HashMap<(i32, i32), Vec<LotId>> = HashMap::new();
        for (id, l) in lots.iter() {
            let (mut lo, mut hi) = (l.center, l.center);
            for p in &l.polygon {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            let (x0, y0) = ((lo.x / cell).floor() as i32, (lo.y / cell).floor() as i32);
            let (x1, y1) = ((hi.x / cell).floor() as i32, (hi.y / cell).floor() as i32);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    cells.entry((x, y)).or_default().push(id);
                }
            }
        }
        Self { cell, cells }
    }

    /// Lots whose bounding box comes within `radius` of `p`, in id order so
    /// ties resolve the way a scan of the map would.
    fn near(&self, p: Vec2, radius: f32) -> Vec<LotId> {
        let (x0, y0) = (((p.x - radius) / self.cell).floor() as i32, ((p.y - radius) / self.cell).floor() as i32);
        let (x1, y1) = (((p.x + radius) / self.cell).floor() as i32, ((p.y + radius) / self.cell).floor() as i32);
        let mut out: Vec<LotId> = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                if let Some(v) = self.cells.get(&(x, y)) {
                    out.extend_from_slice(v);
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// The point of a polyline nearest `p`.
fn closest_on(p: Vec2, pts: &[Vec2]) -> Vec2 {
    let mut best = pts.first().copied().unwrap_or(p);
    for w in pts.windows(2) {
        let d = w[1] - w[0];
        let t = if d.length_squared() > 0.0 { ((p - w[0]).dot(d) / d.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
        let q = w[0] + d * t;
        if q.distance(p) < best.distance(p) {
            best = q;
        }
    }
    best
}

/// A polygon scaled about its centroid, to test overlap without counting
/// outlines that merely touch.
pub(super) fn shrink_about_centroid(poly: &[Vec2], factor: f32) -> Vec<Vec2> {
    let c = crate::zoning::polygon_centroid(poly);
    poly.iter().map(|&p| c + (p - c) * factor).collect()
}

#[cfg(test)]
mod roundabout_tests;
#[cfg(test)]
mod piece_tests;
