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
    /// Stretches of raised road with no room for a column.
    pub pillar_gaps: Vec<crate::pillars::Gap>,
    /// The first of them in words ("Not implemented: ...").
    pub pillar_refusal: Option<String>,
    /// Roads the project lays.
    pub roads: usize,
}
use crate::zoning::{generate_parcels, inner_center, join_parcels, make_lot, Parcel, ParcelKey, ParcelMap};

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
    /// Convenience for scripts and tests: lay track right away.
    pub fn add_rail(&mut self, points: &[Vec2]) -> GestureId {
        let had_project = self.project.take();
        let id = self.add_gesture(Gesture::Road { points: points.to_vec(), lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: RoadKind::Rail, elevation: Vec::new() });
        self.implement();
        self.project = had_project;
        id
    }

    pub fn has_project(&self) -> bool {
        self.project.is_some()
    }

    // ── several projects ────────────────────────────────────────────────

    /// The active project, then the parked ones.
    pub fn all_projects(&self) -> impl Iterator<Item = &Project> {
        self.project.iter().chain(self.parked_projects.iter())
    }

    /// "Project n" for the smallest n not taken.
    fn unused_project_name(&self) -> String {
        (1..).map(|n| format!("Project {n}")).find(|n| !self.all_projects().any(|p| &p.name == n)).unwrap_or_default()
    }

    fn fresh_project(&mut self, name: Option<&str>) -> Project {
        self.next_project = self.next_project.max(1);
        let id = self.next_project;
        self.next_project += 1;
        let name = match name.map(str::trim) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => self.unused_project_name(),
        };
        Project { id, name, ..Project::default() }
    }

    /// Number and name an active project that has neither (a save from
    /// before named projects).
    pub(super) fn name_active_project(&mut self) {
        if self.project.as_ref().is_some_and(|p| p.id == 0) {
            let fresh = self.fresh_project(None);
            let p = self.project.as_mut().unwrap();
            p.id = fresh.id;
            if p.name.is_empty() {
                p.name = fresh.name;
            }
        }
    }

    /// Every pending project, by id, with what it shares with the others.
    pub fn projects(&self) -> Vec<ProjectInfo> {
        let active = self.project.as_ref().map(|p| p.id);
        let mut out: Vec<ProjectInfo> = self
            .all_projects()
            .map(|p| {
                let mut conflicts = Vec::new();
                let mut shared = 0;
                let painted = p.painted();
                for o in self.all_projects().filter(|o| o.id != p.id) {
                    conflicts.extend(p.conflicts_with(o).into_iter().map(|g| (g, o.id)));
                    if !painted.is_empty() {
                        shared += o.painted().intersection(&painted).count();
                    }
                }
                ProjectInfo { id: p.id, name: p.name.clone(), active: Some(p.id) == active, steps: p.steps.len(), redo: p.redo.len(), conflicts, shared_parcels: shared }
            })
            .collect();
        out.sort_by_key(|i| i.id);
        out
    }

    /// Start a new, empty project and make it the active one; the one that
    /// was active is parked. Returns its id.
    pub fn new_project(&mut self, name: Option<&str>) -> u64 {
        self.name_active_project();
        let fresh = self.fresh_project(name);
        let id = fresh.id;
        if let Some(old) = self.project.replace(fresh) {
            self.parked_projects.push(old);
        }
        self.plan_changed();
        id
    }

    /// Make a parked project the active one (the active one is parked).
    pub fn switch_project(&mut self, id: u64) -> bool {
        if self.project.as_ref().is_some_and(|p| p.id == id) {
            return true;
        }
        let Some(i) = self.parked_projects.iter().position(|p| p.id == id) else { return false };
        self.name_active_project();
        let p = self.parked_projects.remove(i);
        if let Some(old) = self.project.replace(p) {
            self.parked_projects.push(old);
        }
        self.parked_projects.sort_by_key(|p| p.id);
        self.plan_changed();
        true
    }

    /// Park the active project: nothing is being edited, every project is
    /// previewed dimmed.
    pub fn park_project(&mut self) -> bool {
        self.name_active_project();
        let Some(p) = self.project.take() else { return false };
        self.parked_projects.push(p);
        self.parked_projects.sort_by_key(|p| p.id);
        self.plan_changed();
        true
    }

    fn project_mut(&mut self, id: u64) -> Option<&mut Project> {
        if self.project.as_ref().is_some_and(|p| p.id == id) {
            return self.project.as_mut();
        }
        self.parked_projects.iter_mut().find(|p| p.id == id)
    }

    pub fn rename_project(&mut self, id: u64, name: &str) -> bool {
        let name = name.trim().to_string();
        if name.is_empty() {
            return false;
        }
        match self.project_mut(id) {
            Some(p) => {
                p.name = name;
                true
            }
            None => false,
        }
    }

    /// Throw a project away, active or parked.
    pub fn delete_project(&mut self, id: u64) -> bool {
        if self.project.as_ref().is_some_and(|p| p.id == id) {
            self.discard_project();
            return true;
        }
        let before = self.parked_projects.len();
        self.parked_projects.retain(|p| p.id != id);
        let gone = self.parked_projects.len() != before;
        if gone {
            self.plan_changed();
        }
        gone
    }

    /// Undo the last step of any project; the others are untouched.
    pub fn undo_project(&mut self, id: u64) -> bool {
        let ok = self.project_mut(id).is_some_and(|p| p.undo());
        if ok {
            self.plan_changed();
        }
        ok
    }

    pub fn redo_project(&mut self, id: u64) -> bool {
        let ok = self.project_mut(id).is_some_and(|p| p.redo());
        if ok {
            self.plan_changed();
        }
        ok
    }

    /// What implementing `project` now would override or be overridden by:
    /// gestures and parcels other pending projects also change. Implemented
    /// in turn, each project's version is built over the last, so the
    /// project implemented later wins.
    fn conflict_warnings(&self, project: &Project) -> Vec<String> {
        let mut out = Vec::new();
        let painted = project.painted();
        let name = |p: &Project| if p.name.is_empty() { "this project".to_string() } else { p.name.clone() };
        for o in self.all_projects().filter(|o| o.id != project.id) {
            let shared = project.conflicts_with(o);
            if !shared.is_empty() {
                let ids: Vec<String> = shared.iter().map(|g| g.0.to_string()).collect();
                out.push(format!("{} and {} both change gesture {}: {}'s version is built now, and {} will build its own over it if implemented later.", name(project), o.name, ids.join(", "), name(project), o.name));
            }
            let both = if painted.is_empty() { 0 } else { o.painted().intersection(&painted).count() };
            if both > 0 {
                out.push(format!("{} and {} both paint {both} parcel{}: the one implemented later decides them.", name(project), o.name, if both == 1 { "" } else { "s" }));
            }
        }
        out
    }

    /// Implement one project (active or parked) over the master plan.
    /// The active project, if it is another, stays active. Returns the
    /// conflict warnings (also left in `plan_warnings`), or None for an
    /// unknown id.
    pub fn implement_project(&mut self, id: u64) -> Option<Vec<String>> {
        if self.project.as_ref().is_some_and(|p| p.id == id) {
            if let Err(why) = self.try_implement() {
                return Some(vec![why]);
            }
            return Some(self.plan_warnings.clone());
        }
        let i = self.parked_projects.iter().position(|p| p.id == id)?;
        let p = self.parked_projects.remove(i);
        let active = self.project.replace(p);
        if let Err(why) = self.try_implement() {
            // Refused: it goes back to waiting where it was.
            if let Some(p) = self.project.take() {
                self.parked_projects.insert(i, p);
            }
            self.project = active;
            self.plan_changed();
            return Some(vec![why]);
        }
        self.project = active;
        self.plan_changed();
        Some(self.plan_warnings.clone())
    }

    /// Every parked project as it would look built over today's master.
    pub fn inactive_previews(&self) -> Vec<ProjectPreview> {
        self.parked_projects
            .iter()
            .map(|p| {
                let effective = p.apply_to(&self.master);
                let zoning = parcel_zoning(&self.parcels, &effective)
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, z)| {
                        let lu = z.map(|(l, _)| l);
                        (lu != self.parcels.parcels[i].land_use).then_some((i, lu))
                    })
                    .collect();
                ProjectPreview { id: p.id, name: p.name.clone(), preview: preview(&self.master, p), zoning }
            })
            .collect()
    }

    // ── parcels and painting ─────────────────────────────────────────────

    /// Land use of every parcel (by index into `parcels`) under the
    /// effective plan: the master with the active project applied. Kept
    /// current with every plan change.
    pub fn planned_zoning(&self) -> &[Option<LandUse>] {
        if self.planned.len() == self.parcels.len() {
            &self.planned
        } else {
            &[]
        }
    }

    pub(super) fn refresh_planned_zoning(&mut self) {
        let z: ParcelZoning = parcel_zoning(&self.parcels, self.effective_plan());
        self.planned = z.into_iter().map(|o| o.map(|(l, _)| l)).collect();
    }

    /// Paint parcels (indices into `parcels`) with a land use, or erase
    /// them back to unzoned with `None`. Only parcels that accept the use
    /// and are not already planned as it are painted (erasing, only zoned
    /// ones). With `stroke` naming the paint gesture of a stroke still
    /// being dragged, the parcels join it and its step is amended, so one
    /// stroke is one undo. Returns the stroke's gesture (None when nothing
    /// was painted and there was no stroke) and how many parcels changed.
    pub fn paint_parcels(&mut self, ids: &[usize], land_use: Option<LandUse>, stroke: Option<GestureId>) -> (Option<GestureId>, usize) {
        self.paint_parcels_with(ids, land_use, None, stroke)
    }

    /// [`World::paint_parcels`] that also sets the industry sub-zone of the
    /// painted ground (`industry`: an industry kind key, or "auto"), applied
    /// on implement. With a sub-zone, parcels already planned industrial are
    /// painted again so their sub-zone changes.
    pub fn paint_parcels_with(&mut self, ids: &[usize], land_use: Option<LandUse>, industry: Option<&str>, stroke: Option<GestureId>) -> (Option<GestureId>, usize) {
        let industry = industry.filter(|_| land_use == Some(LandUse::Industrial)).map(str::to_string);
        let planned = self.planned_zoning();
        let mut keys: Vec<ParcelKey> = Vec::new();
        for &i in ids {
            let Some(p) = self.parcels.parcels.get(i) else { continue };
            let now = planned.get(i).copied().flatten();
            let wanted = self.parcel_refusal(i, land_use).is_none()
                && match land_use {
                    Some(lu) => now != Some(lu) || industry.is_some(),
                    None => now.is_some(),
                };
            if wanted {
                keys.push(p.key);
            }
        }
        keys.sort();
        keys.dedup();
        if keys.is_empty() {
            return (stroke, 0);
        }
        let n = keys.len();
        if let Some(id) = stroke {
            if let Some(Gesture::ParcelZone { parcels: mut have, land_use: l, industry: ind }) = self.gesture(id) {
                if l == land_use && ind == industry {
                    let before = have.len();
                    have.extend(keys);
                    have.sort();
                    have.dedup();
                    // Parcels the stroke already holds are not news.
                    let added = have.len() - before;
                    if added > 0 {
                        self.set_gesture(id, Gesture::ParcelZone { parcels: have, land_use, industry }, false);
                    }
                    return (Some(id), added);
                }
            }
        }
        let id = self.add_gesture(Gesture::ParcelZone { parcels: keys, land_use, industry });
        (Some(id), n)
    }

    /// A zone polygon, and with an industry sub-zone the same parcels
    /// painted with it, as one step (one undo).
    pub fn add_zone_gesture(&mut self, polygon: Vec<Vec2>, land_use: LandUse, industry: Option<&str>) -> GestureId {
        let zone = GestureId(self.next_gesture);
        self.next_gesture += 1;
        let mut changes = vec![(zone, Some(Gesture::Zone { polygon: polygon.clone(), land_use }))];
        if let (LandUse::Industrial, Some(ind)) = (land_use, industry) {
            let mut keys: Vec<ParcelKey> = self.parcels.in_polygon(&polygon).into_iter().filter(|&i| self.parcel_refusal(i, Some(land_use)).is_none()).map(|i| self.parcels.parcels[i].key).collect();
            keys.sort();
            if !keys.is_empty() {
                let id = GestureId(self.next_gesture);
                self.next_gesture += 1;
                changes.push((id, Some(Gesture::ParcelZone { parcels: keys, land_use: Some(land_use), industry: Some(ind.to_string()) })));
            }
        }
        self.edit_project(|p| p.push_many(changes));
        zone
    }

    /// After an implement: the industry sub-zones the project's strokes
    /// painted, one bias per connected patch of their parcels, oldest
    /// stroke first so the newest wins.
    fn apply_industry_strokes(&mut self, project: &Project) {
        let touched: std::collections::BTreeSet<GestureId> = project.touched().collect();
        let mut patches: Vec<(Vec<Vec2>, String)> = Vec::new();
        for id in touched {
            let Some(Gesture::ParcelZone { parcels, land_use: Some(LandUse::Industrial), industry: Some(kind) }) = self.master.gestures.get(&id) else { continue };
            let polys: Vec<Vec<Vec2>> = parcels.iter().filter_map(|k| self.parcels.find(k)).map(|i| self.parcels.parcels[i].polygon.clone()).collect();
            for outline in crate::zoning::union_outlines(&polys) {
                patches.push((outline, kind.clone()));
            }
        }
        for (outline, kind) in patches {
            self.apply_industry_choice(outline, &kind);
        }
    }

    /// Cut the parcels for today's roads: rough ground marked, nothing
    /// under a station, yard or dock footprint.
    fn cut_parcels(&self) -> Vec<Parcel> {
        let water = self.water_mask();
        let h = self.config.half_extent;
        let mut parcels = generate_parcels(&self.roads, &water, Some((Vec2::splat(-h), Vec2::splat(h))));
        if !self.landscape.is_flat() {
            let land = &self.landscape;
            for p in &mut parcels {
                let mut pts = p.polygon.clone();
                pts.push(p.center);
                p.rough = land.is_water(p.center) || land.slope_over(&pts) > crate::landscape::MAX_LOT_SLOPE;
            }
        }
        let footprints: Vec<Vec<Vec2>> = self.facilities.values().map(|f| f.footprint()).chain(self.ports.ports.values().map(|p| p.footprint())).collect();
        parcels.retain(|p| !footprints.iter().any(|fp| point_in_polygon(p.center, fp) || polygons_overlap(&shrink_about_centroid(fp, 0.995), &p.polygon)));
        parcels
    }

    /// Stamp a roundabout into the active project (one undo step). The
    /// centre snaps onto a junction or road within a few metres; radius and
    /// lanes are brought within limits. Errors if another roundabout
    /// already takes the ground.
    pub fn add_roundabout(&mut self, center: Vec2, radius: f32, lanes: u8) -> Result<GestureId, String> {
        let center = self.roundabout_snap(center);
        let ring = crate::roads::Ring::new(center, radius, lanes);
        if crate::plan::ring_effect(self.effective_plan(), ring).blocked {
            return Err("another roundabout already takes this ground".into());
        }
        Ok(self.add_gesture(Gesture::Roundabout { center: ring.center, radius: ring.radius, lanes: ring.lanes }))
    }

    /// Where a roundabout centre dropped at `p` lands: on a junction within
    /// 12 m (that junction becomes the roundabout), else on a street within
    /// 6 m, else `p`.
    pub fn roundabout_snap(&self, p: Vec2) -> Vec2 {
        let junction = self.roads.nodes.iter().enumerate().filter(|(i, n)| n.pos.distance(p) < 12.0 && self.roads.street_degree(NodeId(*i as u32)) >= 3).min_by(|a, b| a.1.pos.distance(p).total_cmp(&b.1.pos.distance(p)));
        if let Some((_, n)) = junction {
            return n.pos;
        }
        match self.roads.nearest_street(p) {
            Some((_, _, q)) if q.distance(p) < 6.0 => q,
            _ => p,
        }
    }

    /// Lots with a building that a roundabout here would take (their
    /// ground reaches under the ring road or its pavements; the inside of
    /// the ring is not taken): demolished on implement, as for any road
    /// works.
    pub fn roundabout_takes(&self, ring: crate::roads::Ring) -> Vec<LotId> {
        let reach = ring.outer_radius();
        let grid = LotGrid::new(&self.lots);
        grid.near(ring.center, reach)
            .into_iter()
            .filter(|&id| {
                let l = &self.lots[id];
                // Only ground under the ring road and its pavements: the
                // inside of the ring stays open land.
                let near = if l.contains(ring.center) { 0.0 } else { crate::geom::distance_to_outline(ring.center, &l.polygon) };
                let far = l.polygon.iter().map(|p| p.distance(ring.center)).fold(0.0, f32::max);
                l.building.is_some() && near < reach && far > ring.inner_radius()
            })
            .collect()
    }

    /// Cut the parcels again and tie each to the saved lot holding its
    /// centre (after a load: lots are saved, parcels are derived).
    pub(super) fn recut_parcels(&mut self) {
        let parcels = self.cut_parcels();
        self.parcels = ParcelMap::new(parcels, self.parcels.version + 1);
        let grid = LotGrid::new(&self.lots);
        for p in &mut self.parcels.parcels {
            let hit = grid.near(p.center, 0.0).into_iter().find(|&id| {
                let l = &self.lots[id];
                self.zones.contains_key(l.zone) && l.contains(p.center)
            });
            if let Some(id) = hit {
                p.lot = Some(id);
                p.land_use = Some(self.lots[id].land_use);
            }
        }
        self.refresh_planned_zoning();
    }

    /// Parcels from the roads, zoning from the master plan, a lot for every
    /// zoned parcel (or run of joined parcels). Zone polygons keep their
    /// zone; painted lots get a zone of their own outline.
    /// `pins` are lots under city buildings: their parcels are the city's
    /// ground whatever is planned, one Civic lot per pin with a zone of its
    /// own (returned), out of the zoning until the building is bulldozed.
    /// `parks` are lots with a park: their parcels
    /// stay park only where the plan leaves them unzoned (a park rezoned
    /// for building is left for development to clear).
    fn build_lots_from_parcels(&mut self, pins: &[Vec<Vec2>], parks: &[Vec<Vec2>]) -> Vec<LotId> {
        let parcels = self.cut_parcels();
        self.parcels = ParcelMap::new(parcels, self.parcels.version + 1);
        self.zones.clear();
        let mut zoning = parcel_zoning(&self.parcels, &self.master);
        // [free-placement] Ground under the city's own buildings is not the
        // zone's, whatever zoning was painted or drawn over it since: those
        // parcels change only once the building is bulldozed.
        let mut pinned_to: Vec<Option<usize>> = vec![None; zoning.len()];
        for (i, z) in zoning.iter_mut().enumerate() {
            let c = self.parcels.parcels[i].center;
            if let Some(k) = pins.iter().position(|poly| point_in_polygon(c, poly)) {
                pinned_to[i] = Some(k);
                *z = None;
            } else if z.is_none() && parks.iter().any(|poly| point_in_polygon(c, poly)) {
                *z = Some((LandUse::Park, GestureId(u64::MAX)));
            }
        }
        // Zones newest first, as they always were listed.
        let zones: Vec<(GestureId, Vec<Vec2>, LandUse)> = self.master.zones().map(|(id, p, lu)| (id, p.to_vec(), lu)).collect();
        let mut zone_of: HashMap<GestureId, ZoneId> = HashMap::new();
        for (gid, polygon, land_use) in zones.into_iter().rev() {
            zone_of.insert(gid, self.zones.insert(Zone { polygon, land_use }));
        }
        for lu in LandUse::ALL {
            let members: Vec<usize> = (0..zoning.len()).filter(|&i| zoning[i].is_some_and(|(l, _)| l == lu)).collect();
            if members.is_empty() {
                continue;
            }
            let polys: Vec<Vec<Vec2>> = members.iter().map(|&i| self.parcels.parcels[i].polygon.clone()).collect();
            for (idx, poly) in join_parcels(&polys, lu, &self.roads) {
                let first = members[idx[0]];
                let Some((_, gid)) = zoning[first] else { continue };
                let center = if idx.len() == 1 { self.parcels.parcels[first].center } else { inner_center(&poly) };
                let zid = match zone_of.get(&gid) {
                    Some(z) => *z,
                    None => self.zones.insert(Zone { polygon: poly.clone(), land_use: lu }),
                };
                let Some(mut lot) = make_lot(zid, lu, poly, center, &self.roads) else { continue };
                if idx.iter().any(|&k| self.parcels.parcels[members[k]].rough) {
                    lot.landlocked = true;
                }
                let id = self.lots.insert(lot);
                for &k in &idx {
                    let p = &mut self.parcels.parcels[members[k]];
                    p.lot = Some(id);
                    p.land_use = Some(lu);
                }
            }
        }
        // [free-placement] One city lot per pin, from the parcels under it.
        let mut pinned = Vec::new();
        for k in 0..pins.len() {
            let members: Vec<usize> = (0..pinned_to.len()).filter(|&i| pinned_to[i] == Some(k)).collect();
            if members.is_empty() {
                continue;
            }
            let polys: Vec<Vec<Vec2>> = members.iter().map(|&i| self.parcels.parcels[i].polygon.clone()).collect();
            let Some(poly) = crate::zoning::joined_outline(&polys) else { continue };
            let center = if members.len() == 1 { self.parcels.parcels[members[0]].center } else { inner_center(&poly) };
            let zid = self.zones.insert(Zone { polygon: poly.clone(), land_use: LandUse::Civic });
            let Some(mut lot) = crate::zoning::make_placed_lot(zid, poly, center, &self.roads) else {
                self.zones.remove(zid);
                continue;
            };
            if members.iter().any(|&i| self.parcels.parcels[i].rough) {
                lot.landlocked = true;
            }
            let id = self.lots.insert(lot);
            for &i in &members {
                let p = &mut self.parcels.parcels[i];
                p.lot = Some(id);
                p.land_use = Some(LandUse::Civic);
            }
            pinned.push(id);
        }
        self.refresh_planned_zoning();
        pinned
    }

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

    /// Make sure there is an active project, starting a fresh one if not.
    pub fn start_project(&mut self) {
        if self.project.is_none() {
            self.project = Some(self.fresh_project(None));
            self.plan_changed();
        }
    }

    pub fn discard_project(&mut self) {
        if self.project.take().is_some() {
            self.plan_changed();
        }
    }

    /// Edit the project (starting one if need be), then refresh the plan.
    pub(super) fn edit_project(&mut self, f: impl FnOnce(&mut Project)) {
        self.start_project();
        f(self.project.as_mut().unwrap());
        self.plan_changed();
    }

    pub fn add_gesture(&mut self, gesture: Gesture) -> GestureId {
        let id = GestureId(self.next_gesture);
        self.next_gesture += 1;
        self.edit_project(|p| p.push(id, Some(gesture)));
        id
    }

    /// Replace a gesture in the project. With `commit == false` the last step
    /// is amended instead of a new one being recorded (continuous drags).
    pub fn set_gesture(&mut self, id: GestureId, gesture: Gesture, commit: bool) {
        self.edit_project(|p| if commit { p.push(id, Some(gesture)) } else { p.amend(id, Some(gesture)) });
    }

    pub fn remove_gesture(&mut self, id: GestureId) {
        self.edit_project(|p| p.push(id, None));
    }

    /// Change the land use of a zone gesture in place.
    /// (Painted parcels are repainted with `paint_parcels`.)
    pub fn repaint_zone(&mut self, id: GestureId, land_use: LandUse) -> bool {
        match self.gesture(id) {
            Some(Gesture::Zone { polygon, .. }) => {
                self.set_gesture(id, Gesture::Zone { polygon, land_use }, true);
                true
            }
            _ => false,
        }
    }

    pub fn undo(&mut self) -> bool {
        let ok = self.project.as_mut().map_or(false, |p| p.undo());
        if ok {
            self.plan_changed();
        }
        ok
    }

    pub fn redo(&mut self) -> bool {
        let ok = self.project.as_mut().map_or(false, |p| p.redo());
        if ok {
            self.plan_changed();
        }
        ok
    }

    /// The master plan with the project applied, as of `plan_version`.
    /// Borrowed, not cloned: it is cached by `plan_changed`.
    pub fn effective_plan(&self) -> &Plan {
        match &self.plan_cache {
            Some((v, p)) if *v == self.plan_version => p,
            // Every mutation refreshes the cache; a miss means `plan_version`
            // was bumped from outside, and the master is the honest fallback.
            _ => &self.master,
        }
    }

    pub fn gesture(&self, id: GestureId) -> Option<Gesture> {
        self.effective_plan().gestures.get(&id).cloned()
    }

    pub fn preview(&self) -> PlanPreview {
        match &self.project {
            Some(p) => preview(&self.master, p),
            None => PlanPreview::default(),
        }
    }

    /// Apply the project to the master plan and rebuild roads, zones, lots and
    /// lanes. Buildings whose lot survives are kept; the rest are demolished.
    ///
    /// Refused (the project stays pending, the reason in
    /// `implement_refused`) while the project's roads would take one of
    /// the city's own structures: see `try_implement`.
    pub fn implement(&mut self) {
        let _ = self.try_implement();
    }

    /// `implement`, saying why when it refuses. Road works may buy out and
    /// pull down what grew, but never a depot, station, plant, service
    /// building, dock, metro station or cargo hub the player placed: a
    /// project that would take one (its roads run over the lot, or leave it
    /// with no street to stand on) is refused until the road is moved or
    /// the structure bulldozed. The tools outline those structures in red
    /// while the road is planned (`project_blockers`).
    pub fn try_implement(&mut self) -> Result<(), String> {
        let Some(project) = self.project.take() else {
            self.implement_refused = None;
            return Ok(());
        };
        if !project.is_empty() {
            // One trial rebuild answers both: the structures the project
            // would take, and [tools] whether every raised road has room
            // for its columns.
            let (blockers, pillars) = self.implement_checks(&project);
            if !blockers.is_empty() {
                let names: Vec<String> = blockers.iter().filter_map(|b| self.buildings.get(*b)).map(|b| b.name.clone()).collect();
                let why = format!("Not implemented: the planned roads would take {}. Move the road clear of it (outlined in red) or bulldoze it first.", names.join(", "));
                self.project = Some(project);
                self.implement_refused = Some(why.clone());
                return Err(why);
            }
            if let Some(why) = pillars {
                self.project = Some(project);
                self.implement_refused = Some(why.clone());
                return Err(why);
            }
        }
        self.implement_refused = None;
        self.plan_warnings = self.conflict_warnings(&project);
        if project.is_empty() {
            self.plan_changed();
            return Ok(());
        }
        self.master = project.apply_to(&self.master);
        self.rebuild_from_master();
        self.apply_industry_strokes(&project);
        Ok(())
    }

    /// The city's own structures (`is_civic_structure`) that the active
    /// project's roads run over: what the road tools outline in red. A
    /// quick test of the planned roads' asphalt against the structures'
    /// lots, cheap enough to run whenever the plan changes.
    pub fn project_blockers(&self) -> Vec<BuildingId> {
        match &self.project {
            Some(p) => self.road_blockers(p),
            None => Vec::new(),
        }
    }

    /// Everything that would refuse `project`: the structures its roads run
    /// over, and (for a project that changes roads at all) any the rebuild
    /// itself would lose, found by implementing it on a copy of the world:
    /// a structure left with no street to front loses its lot too.
    pub fn implement_blockers(&self, project: &Project) -> Vec<BuildingId> {
        self.project_check(project).blockers
    }

    /// [tools] Why `project` cannot be built for want of room for the
    /// columns of a raised road, if so (`project_check`).
    pub fn pillar_refusal(&self, project: &Project) -> Option<String> {
        self.project_check(project).pillar_refusal
    }

    /// [tools] `project_check` of the active project (none: all clear).
    pub fn active_project_check(&self) -> ProjectCheck {
        match &self.project {
            Some(p) if !p.is_empty() => self.project_check(p),
            _ => ProjectCheck::default(),
        }
    }

    fn implement_checks(&self, project: &Project) -> (Vec<BuildingId>, Option<String>) {
        let c = self.project_check(project);
        (c.blockers, c.pillar_refusal)
    }

    /// Everything that would refuse `project`, as implementing checks it:
    /// the structures its roads would take, and [tools] the stretches of
    /// its raised roads (or of a viaduct a road it lays runs under) with no
    /// room for a column. The raised roads are planned as the Road tool
    /// previews them, then the whole project on a copy of the world (the
    /// trial rebuild), so the plan view, the hologram and the refusal all
    /// say the same thing.
    pub fn project_check(&self, project: &Project) -> ProjectCheck {
        let mut out = ProjectCheck { blockers: self.road_blockers(project), ..Default::default() };
        let pv = preview(&self.master, project);
        let roads_change = !pv.planned_roads.is_empty() || !pv.removed_roads.is_empty() || !pv.planned_rings.is_empty() || !pv.removed_rings.is_empty();
        out.roads = pv.planned_roads.len();
        let raised_new = pv.planned_levels.iter().any(|l| crate::pillars::is_raised(l));
        if roads_change && raised_new {
            for ((_, pts, f, b, k), levels) in pv.planned_roads.iter().zip(&pv.planned_levels) {
                out.pillar_gaps.extend(self.road_pillar_plan(pts, levels, *f, *b, *k).gaps);
            }
        }
        let civic = self.buildings.values().any(Self::is_civic_structure);
        let check_pillars = raised_new || !self.pillars.is_empty();
        if roads_change && (civic || check_pillars) {
            let mut trial = self.clone();
            trial.project = None;
            trial.master = project.apply_to(&trial.master);
            trial.rebuild_from_master();
            for (id, b) in self.buildings.iter() {
                if Self::is_civic_structure(b) && !trial.buildings.contains_key(id) && !out.blockers.contains(&id) {
                    out.blockers.push(id);
                }
            }
            if check_pillars {
                for g in &trial.pillar_gaps {
                    if !out.pillar_gaps.iter().any(|o| o.a.distance(g.a) < 3.0 && o.b.distance(g.b) < 3.0) {
                        out.pillar_gaps.push(*g);
                    }
                }
            }
        }
        out.blockers.sort();
        // An end left in the air is the plainer problem: say it first.
        out.pillar_gaps.sort_by_key(|g| !g.end);
        out.pillar_refusal = out.pillar_gaps.first().map(|g| format!("Not implemented: {}.", g.reason()));
        out
    }

    /// The city's own structures whose lots the asphalt of one road would
    /// cover (`pts` drawn at `levels`, as the road tool and the preview
    /// give them): what the road tools outline in red, and what refuses
    /// implementing. Points on a road already built are skipped (a junction
    /// onto the street in front is no trespass), and so are bored tunnels,
    /// which leave the ground alone.
    pub fn structures_in_way(&self, pts: &[Vec2], levels: &[f32], fwd: u8, bwd: u8, kind: RoadKind) -> Vec<BuildingId> {
        let civic: Vec<(BuildingId, &Lot)> = self
            .buildings
            .iter()
            .filter(|(_, b)| Self::is_civic_structure(b))
            .filter_map(|(id, b)| self.lots.get(b.lot).filter(|l| l.building == Some(id) && l.polygon.len() >= 3).map(|l| (id, l)))
            .collect();
        let mut out: Vec<BuildingId> = Vec::new();
        if civic.is_empty() {
            return out;
        }
        let hw = match kind {
            RoadKind::Street => (fwd + bwd).max(2) as f32 * crate::roads::LANE_WIDTH * 0.5 + crate::roads::SHOULDER,
            RoadKind::Path => crate::roads::PATH_HALF_WIDTH,
            RoadKind::Rail => crate::roads::RAIL_HALF_WIDTH,
        };
        for (i, w) in pts.windows(2).enumerate() {
            let (la, lb) = (levels.get(i).copied().unwrap_or(0.0), levels.get(i + 1).copied().unwrap_or(0.0));
            let n = (w[0].distance(w[1]).ceil() as usize).max(1);
            for k in 0..=n {
                let f = k as f32 / n as f32;
                if la + (lb - la) * f <= crate::roads::TUNNEL_LEVEL {
                    continue;
                }
                let p = w[0].lerp(w[1], f);
                if self.roads.nearest_where(p, |s| !s.is_rail()).is_some_and(|(sid, _, q)| q.distance(p) < self.roads.segment(sid).half_width()) {
                    continue;
                }
                for (id, l) in &civic {
                    if out.contains(id) || l.center.distance(p) > hw + l.polygon.iter().map(|c| c.distance(l.center)).fold(0.0, f32::max) {
                        continue;
                    }
                    if point_in_polygon(p, &l.polygon) || crate::geom::distance_to_outline(p, &l.polygon) < hw - 0.5 {
                        // [tools] A deck spanning over it passes over if
                        // it is low enough to fit under (a building of
                        // storeys: a plant, a station or a dock is taller
                        // than its floors say, and is never built over).
                        let storeys = |b: &Building| b.service.is_some() || b.depot || b.metro || b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::CargoHub);
                        if la.min(lb) >= crate::pillars::SPAN_LEVEL && self.buildings.get(*id).is_some_and(|b| storeys(b) && b.floors <= crate::pillars::floors_under(la.min(lb))) {
                            continue;
                        }
                        out.push(*id);
                    }
                }
            }
        }
        out
    }

    /// Civic structures `project`'s new or changed roads (or roundabouts)
    /// would run over (`structures_in_way` for each).
    fn road_blockers(&self, project: &Project) -> Vec<BuildingId> {
        let pv = preview(&self.master, project);
        let mut out: Vec<BuildingId> = Vec::new();
        for ((_, pts, fwd, bwd, kind), levels) in pv.planned_roads.iter().zip(&pv.planned_levels) {
            for id in self.structures_in_way(pts, levels, *fwd, *bwd, *kind) {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
        let civic = |b: &BuildingId| self.buildings.get(*b).is_some_and(Self::is_civic_structure);
        for (_, effect) in &pv.planned_rings {
            if let Some(ring) = effect.ring {
                for lot in self.roundabout_takes(ring) {
                    if let Some(b) = self.lots[lot].building.filter(|b| civic(b) && !out.contains(b)) {
                        out.push(b);
                    }
                }
            }
        }
        out.sort();
        out
    }

    /// Convenience for scripts and tests: build a road right away.
    pub fn add_road(&mut self, points: &[Vec2], lanes_fwd: u8, lanes_bwd: u8) -> GestureId {
        let had_project = self.project.take();
        let id = self.add_gesture(Gesture::Road { points: points.to_vec(), lanes_fwd, lanes_bwd, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        self.implement();
        self.project = had_project;
        id
    }

    /// Convenience for scripts and tests: a walking and cycling path right away.
    pub fn add_path(&mut self, points: &[Vec2]) -> GestureId {
        let had_project = self.project.take();
        let id = self.add_gesture(Gesture::Road { points: points.to_vec(), lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: RoadKind::Path, elevation: Vec::new() });
        self.implement();
        self.project = had_project;
        id
    }

    /// Convenience for scripts and tests: zone right away.
    pub fn add_zone(&mut self, polygon: Vec<Vec2>, land_use: LandUse) -> Option<GestureId> {
        if polygon.len() < 3 {
            return None;
        }
        let had_project = self.project.take();
        let id = self.add_gesture(Gesture::Zone { polygon, land_use });
        self.implement();
        self.project = had_project;
        Some(id)
    }

    pub(super) fn rebuild_from_master(&mut self) {
        self.plan_changed();

        // Roads.
        let mut roads = RoadNetwork::default();
        // Roundabouts cut the roads entering them and join them to their
        // rings; the rings go down first so the arms snap onto them.
        let (built, rings) = self.master.built_roads_levels();
        roads.rings = rings;
        // [tools] The raised ways, for their columns: key, built polyline,
        // levels, deck half width.
        let mut raised: Vec<(u64, Vec<Vec2>, Vec<f32>, f32)> = Vec::new();
        for (_, drawn, levels, fwd, bwd, kind) in built {
            let way = crate::pillars::is_raised(&levels).then(|| crate::pillars::way_key(&drawn, &levels));
            // Track is eased into smooth curves and tangential switches.
            let points = if kind == RoadKind::Rail { rail::smooth_track(&drawn, &roads) } else { drawn.clone() };
            // An end at the map edge runs on out of sight: a way to elsewhere.
            let points = crate::landscape::reach_elsewhere(&points);
            // Ramps meet the highway tangentially: a lead point before a
            // merge (after a diverge) brings the last piece alongside.
            // (A roundabout's ring is a one-way street too, but no ramp.)
            let ring = roads.is_ring_way(&points, fwd, bwd, kind);
            let points = if crate::roads::is_ramp_spec(kind, fwd, bwd) && !ring { crate::roads::ease_ramp_ends(&points, fwd > 0, &roads) } else { points };
            // Breaks at every waterline, so a bridge is exactly the span.
            let points = self.split_at_water(&points);
            // A height sample every so often, so a long straight way follows
            // the land instead of running dead straight between its ends.
            // Only ways that front nothing (highways, track): a street's
            // vertices decide where its parcels are cut.
            let sampled = kind == RoadKind::Rail || (crate::roads::is_highway_spec(kind, fwd, bwd) && !ring);
            let points = if sampled { crate::roads::sample_heights(&points, crate::roads::HEIGHT_SAMPLE) } else { points };
            // Levels follow the points wherever they moved or multiplied.
            let levels = if points == drawn { levels } else { crate::plan::carry_levels(&drawn, &levels, &points) };
            let (fwd, bwd) = match kind {
                RoadKind::Street if fwd == 0 && bwd == 0 => (1, 1),
                RoadKind::Street => (fwd, bwd),
                RoadKind::Path | RoadKind::Rail => (0, 0),
            };
            // [tools] Its ends as the network lays them (an end dropped on a
            // road takes that road's level: a raised road ramps down to it),
            // for its columns, before it is laid.
            let landed = way.map(|_| roads.as_built(&points, &levels, kind));
            roads.add_polyline_levels(&points, &levels, fwd, bwd, kind);
            if let (Some(key), Some((pts, lv))) = (way, landed) {
                raised.push((key, pts, lv, super::deck_half_width(kind, fwd, bwd)));
            }
        }
        // A zoning edit, or a facility fused onto what is there, gives back
        // exactly the roads that are built. Then the network, its ids and
        // everything hanging off them (lanes, vehicles, trains, bus stops,
        // signals, takeover blocks) stay as they are: only lots are re-cut.
        let roads_changed = !same_layout(&self.roads, &roads);
        if roads_changed {
            // Player arrows note where they stand before ids are renumbered.
            self.traffic.anchor_lane_use(&self.roads);
            roads.version = self.roads.version + 1;
            self.roads = roads;
            self.mark_bridges();
            self.rebuild_road_components();
            self.streets.update(&self.roads, self.config.seed);
        } else {
            // Still a new version: the app regrades and redraws the ground
            // and lots on it (lot pads change with the zoning).
            self.roads.version += 1;
        }

        // [tools] Columns under the raised roads, placed against the old
        // lots (they still hold the buildings): grown buildings in the way
        // are bought out below.
        let pillar_taken: std::collections::HashSet<BuildingId> = if roads_changed { self.place_pillars(&raised, true) } else { Default::default() };

        // Parcels along every road, the master's zoning onto them, a lot
        // for every zoned parcel; then old lots with buildings are matched
        // to the new ones. A save from before parcels were cut ahead of
        // zoning has lots of other shapes: its first rebuild moves every
        // building onto the lot that now holds it, without eminent domain.
        let old_lots = std::mem::replace(&mut self.lots, SlotMap::with_key());
        let lenient = !self.parcel_lots;
        self.parcel_lots = true;
        let pins: Vec<Vec<Vec2>> = old_lots.values().filter(|l| l.building.is_some_and(|b| self.is_city_building(b))).map(|l| l.polygon.clone()).collect();
        let parks: Vec<Vec<Vec2>> = old_lots
            .values()
            .filter(|l| l.building.and_then(|b| self.buildings.get(b)).is_some_and(|b| b.land_use == LandUse::Park))
            .map(|l| l.polygon.clone())
            .collect();
        let pinned = self.build_lots_from_parcels(&pins, &parks);
        // Lots on ground too steep to level are left as rough land.
        self.grade_lots();
        // Facilities: their footprints are lots of their own, and zoned lots
        // under them give way.
        // A lot whose centre is under a footprint, or whose outline crosses
        // one, would share ground with the station or yard.
        let footprints: Vec<Vec<Vec2>> = self.facilities.values().map(|f| f.footprint()).chain(self.ports.ports.values().map(|p| p.footprint())).collect();
        self.lots.retain(|_, l| !footprints.iter().any(|fp| point_in_polygon(l.center, fp) || polygons_overlap(&shrink_about_centroid(fp, 0.995), &l.polygon)));
        // [tools] A column takes its lot from the zoning, and so does a deck
        // too low over a lot for a storey under it (parks and the city's
        // own structures stay: a column never stands on one, and a project
        // that would put a deck too low over one is refused).
        if !self.pillars.is_empty() || self.roads.alive_segments().any(|(sid, _)| self.roads.spans_lots(sid)) {
            let drop: Vec<LotId> = self
                .lots
                .iter()
                .filter(|(id, l)| l.land_use != LandUse::Park && !pinned.contains(id))
                .filter(|(_, l)| self.pillars_on(&l.polygon) || self.deck_cap_of(&l.polygon) == Some(0))
                .map(|(id, _)| id)
                .collect();
            for id in drop {
                self.lots.remove(id);
            }
        }
        let fids: Vec<FacilityId> = self.facilities.keys().collect();
        for fid in fids {
            // A facility with no street anywhere to front gets no lot this
            // time round; its building goes with the old lot below.
            self.facilities[fid].lot = self.facility_lot(fid).map(|lot| self.lots.insert(lot));
        }
        let pids: Vec<crate::ports::PortId> = self.ports.ports.keys().collect();
        for pid in pids {
            self.ports.ports[pid].lot = self.port_lot(pid).map(|lot| self.lots.insert(lot));
        }
        // Match old lots to new ones through a coarse grid of the new lots'
        // bounding boxes, so a rebuild costs lots × neighbours, not lots².
        let grid = LotGrid::new(&self.lots);
        // (building, where it stood, pulled down for the works?)
        let mut demolish: Vec<(BuildingId, Vec2, bool)> = Vec::new();
        // Old lot key to the new lot its building moved onto.
        let mut carried: HashMap<LotId, LotId> = HashMap::new();
        // [land] A building over merged plots names each of them; only its
        // primary lot moves the building, the others follow as extras.
        let primary_of: HashMap<BuildingId, LotId> = self.buildings.iter().map(|(id, b)| (id, b.lot)).collect();
        let mut extras: HashMap<BuildingId, Vec<LotId>> = HashMap::new();
        for (old_id, old) in old_lots.iter() {
            let Some(bid) = old.building else { continue };
            if primary_of.get(&bid).is_some_and(|p| *p != old_id) {
                let found = grid
                    .near(old.center, 6.0)
                    .into_iter()
                    .filter(|id| self.lots[*id].building.is_none() && self.lots[*id].contains(old.center))
                    .find(|id| !outlines_differ(&old.polygon, &self.lots[*id].polygon, EXPROPRIATE_AREA_TOL, EXPROPRIATE_DIST_TOL));
                if let Some(new_lot) = found {
                    carried.insert(old_id, new_lot);
                    self.lots[new_lot].building = Some(bid);
                    extras.entry(bid).or_default().push(new_lot);
                }
                continue;
            }
            let found = grid
                .near(old.center, 6.0)
                .into_iter()
                .map(|id| (id, &self.lots[id]))
                .filter(|(_, l)| l.building.is_none() && l.usable())
                .map(|(id, l)| (id, if l.contains(old.center) { 0.0 } else { l.center.distance(old.center) }))
                .filter(|(_, d)| *d < 6.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id);
            // [free-placement] City buildings (city parking too) stay on the
            // city lot under them.
            let civic = self.is_city_building(bid);
            let floors = self.buildings.get(bid).map_or(0, |b| b.floors);
            match found {
                // [tools] Bought out for a column, or too tall for the deck
                // now spanning over it.
                _ if !civic && pillar_taken.contains(&bid) => demolish.push((bid, old.center, true)),
                Some(new_lot) if !civic && self.deck_cap(new_lot).is_some_and(|cap| floors > cap) => demolish.push((bid, old.center, true)),
                // Eminent domain: a parcel reshaped by road or path works
                // cannot keep its building, even if a lot is still there.
                Some(new_lot)
                    if !lenient
                        && !civic
                        && outlines_differ(&old.polygon, &self.lots[new_lot].polygon, EXPROPRIATE_AREA_TOL, EXPROPRIATE_DIST_TOL) =>
                {
                    demolish.push((bid, old.center, true));
                }
                Some(new_lot) => {
                    carried.insert(old_id, new_lot);
                    self.lots[new_lot].building = Some(bid);
                    if let Some(b) = self.buildings.get_mut(bid) {
                        b.lot = new_lot;
                    }
                }
                None => demolish.push((bid, old.center, false)),
            }
        }
        // [land] Extras follow their building; bought plots still waiting
        // are let go (the works changed them).
        for (bid, b) in self.buildings.iter_mut() {
            b.plot.extra_lots = extras.remove(&bid).unwrap_or_default();
            b.plot.pending_lots.clear();
        }
        self.estate.reserved.clear();
        for (bid, at, expropriated) in demolish {
            if expropriated {
                let lot = self.buildings.get(bid).map(|b| b.lot);
                if let Some(lot) = lot {
                    self.events.push(SimEvent::Expropriated(bid, lot));
                }
                self.demolish_with(bid, Some(at), Some("Our block was pulled down for the road works. Homeless again."));
            } else {
                self.demolish_with(bid, Some(at), None);
            }
        }

        // Everything else that names a lot by key follows it to its new key.
        self.remap_lot_keys(&old_lots, &grid, &carried);
        // [free-placement] City buildings' lots are the city's (an older
        // save's first rebuild switches them); city ground whose building did
        // not survive the works goes back to the plan.
        self.settle_city_ground(&pinned, false);

        // Road heights and the ground they are graded into; new bridges are paid for.
        self.rebuild_ground();
        self.charge_bridges();

        if roads_changed {
            self.carry_traffic();
            // Trains ran on track that may be gone or re-laid, and their stops
            // and node ids are stale either way: everyone gets off where the
            // train stands and the next headway brings fresh ones.
            self.clear_trains();
            self.resnap_bus_stops();
        }
        // [carlife] Parked cars on streets that changed find a kerb again.
        self.refresh_kerbs();
        self.refresh_rail_links();
        self.refresh_port_links();
        self.rebuild_walk();
        // Coverage is per lot key: the old map names other lots now.
        self.refresh_coverage();

        self.events.push(SimEvent::RoadsChanged);
        self.events.push(SimEvent::LotsChanged);
        self.events.push(SimEvent::PlanChanged);
    }

    /// Lanes for the new roads, with every vehicle carried across them
    /// (`traffic/remap.rs`): a vehicle on a lane that did not change drives
    /// on as it was, one on a reshaped lane goes on from the new lane under
    /// it. Only one whose street was removed, or that has no way on from
    /// where it stands, leaves the road: through traffic rejoins what is
    /// left of the highway, anything else parks or is stranded there.
    pub(super) fn carry_traffic(&mut self) {
        let report = self.traffic.rebuild_carrying(&self.roads);
        self.apply_tolls();
        // [events] Wear follows the pieces that are still there.
        self.apply_road_wear();
        for (id, car) in report.lost {
            // (A service unit only looks like through traffic: its station
            // recovers it.)
            if Self::is_through_car(&car) && self.services.unit_of_car(id).is_none() && self.resume_through_car(&car) {
                continue;
            }
            self.strand_car(car);
        }
    }

    /// Bus stops follow the streets they sat on, or go. A stop that goes is
    /// taken out the way the player removing it would be
    /// (`stop_removed`), so buses and riders bound for later stops keep
    /// their places on the line.
    pub(super) fn resnap_bus_stops(&mut self) {
        let ids: Vec<RouteId> = self.transit.routes.keys().collect();
        for rid in ids {
            let snapped: Vec<Option<BusStop>> = self.transit.routes[rid].stops.iter().map(|s| BusStop::snap(&self.roads, s.pos)).collect();
            for (k, s) in snapped.iter().enumerate() {
                if let Some(s) = s {
                    self.transit.routes[rid].stops[k] = *s;
                }
            }
            for k in (0..snapped.len()).rev() {
                if snapped[k].is_none() {
                    self.transit.routes[rid].stops.remove(k);
                    self.stop_removed(rid, k);
                }
            }
        }
        self.transit.refresh_legs(&self.traffic.net);
    }

    /// Every rebuild cuts lots afresh, and a fresh SlotMap hands out the
    /// same keys again in a new order: an old key held anywhere now names
    /// some other lot (and no `.get()` guard notices). Each old key is
    /// mapped to the new lot its building moved onto, else the new lot
    /// under its old centre, and the keys kept elsewhere follow: rubble by
    /// where it lies, camps and footfall still owed to shops by their lot.
    /// Keys with no lot any more are dropped.
    fn remap_lot_keys(&mut self, old_lots: &SlotMap<LotId, Lot>, grid: &LotGrid, carried: &HashMap<LotId, LotId>) {
        let lots = &self.lots;
        let lot_at = |p: Vec2| -> Option<LotId> {
            let near = grid.near(p, 6.0);
            near.iter().copied().find(|&id| lots.get(id).is_some_and(|l| l.contains(p))).or_else(|| {
                near.iter()
                    .filter_map(|&id| lots.get(id).map(|l| (id, l.center.distance(p))))
                    .filter(|(_, d)| *d < 6.0)
                    .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
                    .map(|(id, _)| id)
            })
        };
        let map: HashMap<LotId, LotId> = old_lots.iter().filter_map(|(id, l)| carried.get(&id).copied().or_else(|| lot_at(l.center)).map(|n| (id, n))).collect();
        let rubble = std::mem::take(&mut self.services.rubble);
        let rubble: Vec<(LotId, Vec2, u64)> = rubble.into_iter().filter_map(|(lot, at, until)| lot_at(at).or_else(|| map.get(&lot).copied()).map(|n| (n, at, until))).collect();
        self.services.rubble = rubble;
        for camp in &mut self.society.homeless.camps {
            camp.lot = camp.lot.and_then(|l| map.get(&l).copied());
        }
        for c in self.citizens.values_mut() {
            if !c.pending_footfall.is_empty() {
                c.pending_footfall = c.pending_footfall.iter().filter_map(|l| map.get(l).copied()).collect();
            }
        }
    }

    /// Union-find over the alive street and path segments: two nodes share
    /// a component when a walker or car can get from one to the other.
    pub(super) fn rebuild_road_components(&mut self) {
        let n = self.roads.nodes.len();
        let mut parent: Vec<u32> = (0..n as u32).collect();
        fn find(parent: &mut [u32], mut x: u32) -> u32 {
            while parent[x as usize] != x {
                let p = parent[x as usize];
                parent[x as usize] = parent[p as usize];
                x = p;
            }
            x
        }
        for (_, seg) in self.roads.alive_segments() {
            if seg.is_rail() {
                continue;
            }
            let (a, b) = (find(&mut parent, seg.a.0), find(&mut parent, seg.b.0));
            if a != b {
                parent[a as usize] = b;
            }
        }
        self.road_component = (0..n as u32).map(|i| find(&mut parent, i)).collect();
    }

    /// Component id of a road node, if the node exists.
    pub(super) fn road_component(&self, n: NodeId) -> Option<u32> {
        self.road_component.get(n.0 as usize).copied()
    }

    /// Take every train off the line; its passengers stand where it was
    /// and decide again. [freight] Goods aboard go back out of town, first
    /// in the queue for the next freight train (or by road, should their
    /// yard no longer work: `drain_stranded_rail_imports`).
    pub(super) fn clear_trains(&mut self) {
        let trains: Vec<Train> = self.rail.trains.drain().map(|(_, t)| t).collect();
        let since = self.clock.tick.saturating_sub(crate::rail::FREIGHT_HEADWAY);
        for t in trains {
            self.rail.orders.extend(t.cargo.iter().map(|c| crate::rail::RailOrder { cargo: *c, since }));
            let pos = t.head();
            for p in t.passengers {
                if let Some(c) = self.citizens.get_mut(p) {
                    c.transit = None;
                }
                self.strand_driver(p, pos);
            }
        }
    }

    /// Is this one of the city's own structures (a plant, depot, station
    /// or yard)? Those are never taken over by another placement; the player
    /// bulldozes them first.
    /// A parcel under one of the city's own structures (or a station or
    /// yard footprint): its zoning cannot change until the structure goes.
    pub fn parcel_holds_civic(&self, i: usize) -> bool {
        let Some(p) = self.parcels.parcels.get(i) else { return false };
        if self.under_facility(p.center) {
            return true;
        }
        self.lots.values().any(|l| l.contains(p.center) && self.is_city_ground(l))
    }

    /// Why parcel `i` cannot be zoned `lu` (or unzoned, with None): a city
    /// structure standing on it, then the parcel's own rules
    /// (`Parcel::refusal`).
    pub fn parcel_refusal(&self, i: usize, lu: Option<LandUse>) -> Option<&'static str> {
        let p = self.parcels.parcels.get(i)?;
        if self.parcel_holds_civic(i) {
            return Some("a city building stands here: bulldoze it first");
        }
        lu.and_then(|lu| p.refusal(lu))
    }

    pub(super) fn is_civic_structure(b: &Building) -> bool {
        b.is_power() || b.depot || b.rail.is_some() || b.port.is_some() || b.service.is_some() || b.metro || b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::CargoHub)
    }

    /// The city takes a lot for one of its own structures: whatever grew
    /// there (homes, a shop, a site still going up) is bought out and pulled
    /// down, its residents and staff move on with a memory of it. Another
    /// civic structure is not replaced: that is an error the player sees.
    pub(super) fn take_over_lot(&mut self, lot_id: LotId, for_what: &str) -> Result<(), String> {
        let Some(bid) = self.lots.get(lot_id).and_then(|l| l.building) else { return Ok(()) };
        let Some(b) = self.buildings.get(bid) else { return Ok(()) };
        if Self::is_civic_structure(b) {
            return Err(format!("{} stands there; bulldoze it first", b.name));
        }
        self.check_take_over(bid)?;
        let memory = format!("The city bought out {} to build a {for_what}. Had to move on.", b.name);
        self.events.push(SimEvent::Expropriated(bid, lot_id));
        self.demolish_with(bid, None, Some(&memory));
        Ok(())
    }

    /// Hook for refusing a take-over (none today: the city may take any
    /// grown building).
    fn check_take_over(&self, _bid: BuildingId) -> Result<(), String> {
        Ok(())
    }

    pub(super) fn demolish(&mut self, bid: BuildingId) {
        self.demolish_with(bid, None, None);
    }

    /// Take a building down. `at` is where it stood when its lot is already
    /// gone; `memory` replaces the residents' default note about it.
    pub(super) fn demolish_with(&mut self, bid: BuildingId, at: Option<Vec2>, memory: Option<&str>) {
        // Nothing vanishes: it becomes a demolition site and is cleared over
        // the next days (see `construction.rs`), unless it must go at once.
        if self.begin_demolition(bid, at, memory) {
            return;
        }
        let city = self.is_city_building(bid);
        let Some(b) = self.buildings.remove(bid) else { return };
        if at.is_none() && self.lots.get(b.lot).is_some_and(|l| l.building == Some(bid)) {
            self.lots[b.lot].building = None;
            // [free-placement] Its ground goes back to the plan.
            if city {
                self.release_city_lot(b.lot, true);
            }
        }
        let tick = self.clock.tick;
        let center = self.lots.get(b.lot).map(|l| l.center).or(at).unwrap_or(Vec2::ZERO);
        for cid in b.residents.iter().copied() {
            if let Some(c) = self.citizens.get_mut(cid) {
                if c.home == Some(bid) {
                    c.home = None;
                    match memory {
                        Some(m) => c.remember(tick, m.to_string()),
                        None => c.remember(tick, format!("{} was demolished. Homeless again.", b.name)),
                    }
                    self.concerns.heard(cid, c, crate::concerns::Concern::Displacement, 0.8);
                }
            }
        }
        // Households too, not only their members: the household's home
        // would otherwise dangle until the hourly `sync_households`.
        for hid in b.households.iter().copied() {
            if let Some(h) = self.households.get_mut(hid) {
                if h.home == Some(bid) {
                    h.home = None;
                }
            }
        }
        for cid in b.workers.iter().copied() {
            if self.citizens.get(cid).is_some_and(|c| c.work == Some(bid)) {
                self.lay_off(cid, None);
            }
        }
        let inside: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.inside == Some(bid)).map(|(id, _)| id).collect();
        for cid in inside {
            let c = &mut self.citizens[cid];
            c.inside = None;
            c.pos = center;
            c.activity = Activity::Idle;
        }
        if b.rail.is_some() {
            for f in self.facilities.values_mut() {
                if f.building == Some(bid) {
                    f.building = None;
                }
            }
            self.refresh_facility_cache();
        }
        self.events.push(SimEvent::BuildingRemoved(bid, b.lot));
        // [land] Extra plots freed, a bought plot handed over.
        let lot = if at.is_none() { b.lot } else { LotId::default() };
        self.estate_building_gone(bid, &b.plot, lot);
    }

    /// Someone loses their job: back to gig money, with a note if there is
    /// a story to it.
    pub(super) fn lay_off(&mut self, cid: CitizenId, memory: Option<String>) {
        let tick = self.clock.tick;
        if let Some(c) = self.citizens.get_mut(cid) {
            c.work = None;
            c.income = GIG_INCOME;
            if let Some(m) = memory {
                // [concerns] A shop going under hurts; a site crew moving on is routine.
                let s = if m.contains("shut down") { 0.6 } else { 0.15 };
                c.remember(tick, m);
                self.concerns.heard(cid, c, crate::concerns::Concern::Jobs, s);
            }
        }
    }

    /// Personal skill factor, fixed per person, spreading incomes within a job.
    pub(super) fn skill_of(seed: u64) -> f32 {
        0.75 + (seed % 1000) as f32 / 1000.0 * 0.6
    }

    /// A car taken off the road: its driver, or a bus's riders, stand there.
    pub(super) fn strand_car(&mut self, car: crate::traffic::Car) {
        use crate::vehicle::{CarEvent, Errand};
        // A truck taken off the road still delivers: the buyer already counts
        // the load as inbound and would never reorder it otherwise.
        if let Some(cargo) = car.cargo {
            self.deliver(cargo);
        }
        // [carlife] Trucks and vans are back at their base; private cars
        // park where they stood (cars on their way to be sold or out of
        // town are gone, as they were about to be).
        match car.errand {
            Errand::Haul { base: Some(b) } | Errand::Return(b) if self.buildings.contains_key(b) => {
                *self.vehicles.fleet_parked.entry(b).or_insert(0) += 1;
            }
            _ => {}
        }
        if car.cargo.is_some() {
            return;
        }
        if let Some(vid) = car.owned.filter(|v| self.vehicles.cars.contains_key(*v)) {
            match car.errand {
                Errand::Sale(_) => self.drop_car(vid, CarEvent::Sold),
                Errand::Leave => self.drop_car(vid, CarEvent::Emigrated),
                _ => self.park_stranded(vid, car.pos),
            }
        }
        if let Some(bus) = &car.bus {
            for &p in &bus.passengers {
                if let Some(c) = self.citizens.get_mut(p) {
                    c.transit = None;
                }
                self.strand_driver(p, car.pos);
            }
        } else {
            self.strand_driver(car.citizen, car.pos);
        }
    }

    /// Whoever was in a vehicle that vanished (network rebuilt, bad save)
    /// stands where it was and decides again.
    pub(super) fn strand_driver(&mut self, cid: CitizenId, pos: Vec2) {
        if let Some(c) = self.citizens.get_mut(cid) {
            c.pos = pos;
            c.inside = None;
            c.activity = Activity::Idle;
            c.pending_footfall.clear();
            c.next_decision_tick = self.clock.tick + 1;
        }
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

/// A polygon scaled about its centroid, to test overlap without counting
/// outlines that merely touch.
pub(super) fn shrink_about_centroid(poly: &[Vec2], factor: f32) -> Vec<Vec2> {
    let c = crate::zoning::polygon_centroid(poly);
    poly.iter().map(|&p| c + (p - c) * factor).collect()
}

#[cfg(test)]
#[path = "roundabout_tests.rs"]
mod roundabout_tests;
