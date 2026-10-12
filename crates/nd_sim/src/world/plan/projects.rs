//! Projects: the player's drafts of road and zoning work, their gestures,
//! undo and redo, and the plan they make with the master.

use super::*;

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

    /// The active project, then the parked ones.
    pub fn all_projects(&self) -> impl Iterator<Item = &Project> {
        self.project.iter().chain(self.parked_projects.iter())
    }

    /// "Project n" for the smallest n not taken.
    pub(super) fn unused_project_name(&self) -> String {
        (1..).map(|n| format!("Project {n}")).find(|n| !self.all_projects().any(|p| &p.name == n)).unwrap_or_default()
    }

    pub(super) fn fresh_project(&mut self, name: Option<&str>) -> Project {
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
    pub(in crate::world) fn name_active_project(&mut self) {
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

    pub(super) fn project_mut(&mut self, id: u64) -> Option<&mut Project> {
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
    pub(super) fn conflict_warnings(&self, project: &Project) -> Vec<String> {
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
    pub(in crate::world) fn edit_project(&mut self, f: impl FnOnce(&mut Project)) {
        self.start_project();
        f(self.project.as_mut().unwrap());
        self.plan_changed();
    }

    pub fn add_gesture(&mut self, gesture: Gesture) -> GestureId {
        // [parks] (D5) A path drawn into a park's interior (its hub gate)
        // ends at the park's edge gate: the park walks it on to the hub.
        let gesture = match gesture {
            Gesture::Road { points, lanes_fwd, lanes_bwd, curved, kind: RoadKind::Path, elevation } => Gesture::Road { points: self.path_into_parks(&points), lanes_fwd, lanes_bwd, curved, kind: RoadKind::Path, elevation },
            g => g,
        };
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

    /// [fix/roads-tools] Replace several gestures as one step (a junction
    /// dragged with every road attached to it). With `commit == false` the
    /// last step is amended if it touches exactly these gestures.
    pub fn set_gestures(&mut self, changes: Vec<(GestureId, Gesture)>, commit: bool) {
        let changes: Vec<(GestureId, Option<Gesture>)> = changes.into_iter().map(|(id, g)| (id, Some(g))).collect();
        self.edit_project(|p| if commit { p.push_many(changes) } else { p.amend_many(changes) });
    }

    pub fn remove_gesture(&mut self, id: GestureId) {
        self.edit_project(|p| p.push(id, None));
    }

    /// [pt-ux] The stretch of road `id` a bulldozer click at `p` takes: from
    /// junction to junction (or road end), as metres along its built
    /// polyline. `piece` is the built street piece under the click, if any;
    /// its run between junctions (`road_run`) is the stretch. A road only
    /// planned, or a click the network does not back, takes the whole road.
    pub fn road_piece_span(&self, id: GestureId, piece: Option<crate::roads::SegmentId>, p: Vec2) -> Option<(f32, f32)> {
        let g = self.gesture(id)?;
        if !matches!(g, Gesture::Road { .. }) {
            return None;
        }
        let line = g.road_polyline();
        let total: f32 = line.windows(2).map(|w| w[0].distance(w[1])).sum();
        let whole = Some((0.0, total));
        let Some(sid) = piece.filter(|s| self.roads.try_segment(*s).is_some_and(|s| s.alive)) else { return whole };
        // The piece must lie on this road (not a neighbour met at a joint).
        let seg = self.roads.segment(sid);
        let mid = (self.roads.node(seg.a).pos + self.roads.node(seg.b).pos) * 0.5;
        if crate::geom::distance_to_polyline(mid, &line) > 3.0 || crate::geom::distance_to_polyline(p, &line) > 30.0 {
            return whole;
        }
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for s in self.road_run(sid) {
            let s = self.roads.segment(s);
            for n in [s.a, s.b] {
                let q = self.roads.node(n).pos;
                // A run carrying on into another road ends where this one does.
                if crate::geom::distance_to_polyline(q, &line) > 3.0 {
                    continue;
                }
                let a = crate::plan::arc_of(&line, q);
                lo = lo.min(a);
                hi = hi.max(a);
            }
        }
        if !(lo < hi) {
            return whole;
        }
        Some((lo, hi))
    }

    /// [pt-ux] Take the stretch `s0`..`s1` metres along road `id` out of the
    /// plan, leaving the road either side (one undo step). The piece after
    /// the gap is a new gesture. How many roads are left of it.
    pub fn remove_road_piece(&mut self, id: GestureId, s0: f32, s1: f32) -> usize {
        let Some(g) = self.gesture(id) else { return 0 };
        let parts = g.cut_out(s0, s1);
        let left = parts.len();
        let mut it = parts.into_iter();
        let mut changes = vec![(id, it.next())];
        for rest in it {
            let nid = GestureId(self.next_gesture);
            self.next_gesture += 1;
            changes.push((nid, Some(rest)));
        }
        self.edit_project(|p| p.push_many(changes));
        left
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
}
