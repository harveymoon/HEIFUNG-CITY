//! [tools] Implementing, and what would refuse it: the structures a
//! project's roads take and the columns its raised roads need room for,
//! found on a trial rebuild.

use super::*;

impl World {
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
            // for its columns. The plan view shows this same check.
            if let Some(why) = self.project_check(&project).refusal {
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
        let before = std::mem::take(&mut self.master);
        self.master = project.apply_to(&before);
        // [w1-roads] Curved roads through the project's junctions get their
        // point there before they are built.
        let touched: std::collections::BTreeSet<GestureId> = self.master.gestures.iter().filter(|(id, g)| before.gestures.get(*id) != Some(*g)).map(|(id, _)| *id).collect();
        drop(before);
        self.mend_curves_through(&touched);
        // [r5-fixes] Pockets of streets with no way out the works make.
        let traps_before = self.roads.traps();
        self.rebuild_from_master();
        // [fix/roads-tools] Roads meeting at a junction share one point there.
        self.mend_junctions();
        self.apply_industry_strokes(&project);
        let made = new_traps(&traps_before, &self.roads.traps());
        self.plan_warnings.extend(made.iter().map(|t| format!("Warning: {}.", crate::roads::trap_warning(t))));
        Ok(())
    }

    /// [r5-fixes] The Road tool's warning while a street is drawn: the
    /// pockets of streets with no way out (one-way streets leading into
    /// streets nothing leads out of) that laying it would make, in words.
    /// Traffic is never routed into such a pocket (`LaneNetwork::trap`),
    /// so whatever stands there gets no deliveries, visits or services.
    pub fn road_trap_warnings(&self, points: &[Vec2], levels: &[f32], kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8) -> Vec<String> {
        self.road_traps(points, levels, kind, lanes_fwd, lanes_bwd).iter().map(crate::roads::trap_warning).collect()
    }

    /// [r5-fixes] `road_trap_warnings`' pockets.
    pub fn road_traps(&self, points: &[Vec2], levels: &[f32], kind: RoadKind, lanes_fwd: u8, lanes_bwd: u8) -> Vec<crate::roads::Trap> {
        if kind != RoadKind::Street || points.len() < 2 || lanes_fwd + lanes_bwd == 0 {
            return Vec::new();
        }
        let before = self.roads.traps();
        let mut roads = self.roads.clone();
        roads.add_polyline_levels(points, levels, lanes_fwd, lanes_bwd, kind);
        new_traps(&before, &roads.traps())
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

    /// Everything that would refuse `project`, as implementing checks it:
    /// the structures its roads would take, and [tools] the stretches of
    /// its raised roads (or of a viaduct a road it lays runs under) with no
    /// room for a column. The raised roads are planned as the Road tool
    /// previews them, then the whole project on a copy of the world (the
    /// trial rebuild), so the plan view, the hologram and the refusal all
    /// say the same thing.
    /// Whether a stretch with no room for a column lies on or under one of
    /// `roads` (within a wide deck's width of it).
    fn gap_touches(g: &crate::pillars::Gap, roads: &[&[Vec2]]) -> bool {
        const NEAR: f32 = 16.0;
        roads.iter().any(|pts| {
            if pts.len() == 1 {
                return crate::geom::segment_distance_between(g.a, g.b, pts[0], pts[0]) < NEAR;
            }
            pts.windows(2).any(|w| crate::geom::segment_distance_between(g.a, g.b, w[0], w[1]) < NEAR)
        })
    }

    pub fn project_check(&self, project: &Project) -> ProjectCheck {
        let pv = preview(&self.master, project);
        let mut out = ProjectCheck { blocks: self.road_blocks(&pv), ..Default::default() };
        let roads_change = !pv.planned_roads.is_empty() || !pv.removed_roads.is_empty() || !pv.planned_rings.is_empty() || !pv.removed_rings.is_empty();
        out.roads = pv.planned_roads.len();
        let raised_new = pv.planned_levels.iter().any(|l| crate::pillars::is_raised(l));
        let replaced: Vec<Vec<Vec2>> = pv.removed_roads.iter().map(|(_, pts, ..)| pts.clone()).collect();
        if roads_change && raised_new {
            for ((_, pts, f, b, k), levels) in pv.planned_roads.iter().zip(&pv.planned_levels) {
                out.pillar_gaps.extend(self.road_pillar_plan_replacing(pts, levels, *f, *b, *k, &replaced).gaps);
            }
        }
        // [fix/roads-tools] Any building: the trial also says which grown
        // buildings the works would take, for the red outlines.
        let civic = !self.buildings.is_empty();
        let check_pillars = raised_new || !self.pillars.is_empty();
        if roads_change && (civic || check_pillars) {
            let mut trial = crate::prof!("check.clone", self.clone());
            trial.project = None;
            trial.master = project.apply_to(&trial.master);
            trial.rebuild_from_master_with(true);
            for (id, b) in self.buildings.iter() {
                if Self::is_civic_structure(b) && !trial.buildings.contains_key(id) && !out.blocks.iter().any(|k| k.building == id) {
                    out.blocks.push(self.lost_block(id, &pv));
                } else if !Self::is_civic_structure(b) && !trial.buildings.get(id).is_some_and(|t| t.demolition.is_none()) && b.demolition.is_none() {
                    out.lost.push(id);
                }
            }
            if check_pillars {
                // Only gaps the project's own roads touch refuse it: a
                // viaduct already built over buildings that grew under it
                // later is the city's, and must not block every road drawn
                // anywhere else (Harvey's day-9 save: no road could be
                // implemented at all).
                let changed: Vec<&[Vec2]> = pv.planned_roads.iter().map(|(_, pts, ..)| &pts[..]).chain(replaced.iter().map(|p| &p[..])).collect();
                for g in trial.pillar_gaps.iter().filter(|g| Self::gap_touches(g, &changed)) {
                    if !out.pillar_gaps.iter().any(|o| o.a.distance(g.a) < 3.0 && o.b.distance(g.b) < 3.0) {
                        out.pillar_gaps.push(*g);
                    }
                }
            }
        }
        out.blocks.sort_by_key(|k| k.building);
        out.blockers = out.blocks.iter().map(|k| k.building).collect();
        // An end left in the air is the plainer problem: say it first.
        out.pillar_gaps.sort_by_key(|g| !g.end);
        out.pillar_refusal = out.pillar_gaps.first().map(|g| format!("Not implemented: {}.", g.reason()));
        // [dams] (H1) No road on a dam: dams carry footpaths only.
        out.dam_refusal = pv.planned_roads.iter().enumerate().find_map(|(i, (_, pts, f, b, k))| {
            let levels = pv.planned_levels.get(i).map_or(&[][..], |l| &l[..]);
            self.road_over_dam(pts, levels, *k, crate::roads::half_width_spec(*k, *f, *b))
        });
        // [r1-roads] (K12, K13) A road changed in the Edit tool (raised,
        // lowered) is held to the Road tool's rules: where it crosses a way
        // at a height that is neither a junction nor clear of it, it is
        // refused with the reason, not built through the other.
        out.crossing_refusal = pv.planned_roads.iter().enumerate().find_map(|(i, (_, pts, f, b, k))| {
            let levels = pv.planned_levels.get(i).map_or(&[][..], |l| &l[..]);
            self.crossing_refusal(pts, levels, *k, *f, *b, &replaced).map(|why| format!("Not implemented: {} ({}).", why, self.planned_road_label(pts)))
        });
        out.refusal = Self::blocks_refusal(&out.blocks).or_else(|| out.dam_refusal.clone()).or_else(|| out.crossing_refusal.clone()).or_else(|| out.pillar_refusal.clone());
        out
    }

    /// The refusal for structures a project would take, naming the first
    /// with the road that takes it.
    pub(super) fn blocks_refusal(blocks: &[Blocker]) -> Option<String> {
        let first = blocks.first()?;
        let more = match blocks.len() {
            1 => String::new(),
            2 => " (and 1 more structure)".to_string(),
            n => format!(" (and {} more structures)", n - 1),
        };
        let hint = if first.runs_over { "Move the road clear of it (outlined in red) or bulldoze it first." } else { "Leave it a street to front, or bulldoze it first (outlined in red)." };
        Some(format!("Not implemented: {}{more}. {hint}", first.reason()))
    }

    /// [tools] A planned road in words for a refusal: "the new road off
    /// Pylon Drive (from (x, y))" when it joins a named street, else where
    /// it starts.
    pub(super) fn planned_road_label(&self, pts: &[Vec2]) -> String {
        let start = pts.first().copied().unwrap_or_default();
        let joins = |p: Vec2| {
            self.roads
                .nearest_where(p, |s| !s.is_rail())
                .filter(|(sid, _, q)| q.distance(p) < self.roads.segment(*sid).half_width() + 2.0)
                .and_then(|(sid, _, _)| self.streets.of(sid))
                .map(|st| st.name.clone())
        };
        match pts.iter().copied().find_map(joins) {
            Some(name) => format!("the new road off {name} (from ({:.0}, {:.0}))", start.x, start.y),
            None => format!("the new road from ({:.0}, {:.0})", start.x, start.y),
        }
    }

    /// [tools] A road the project takes out, in words: "taking out Pylon
    /// Drive" (the street it is today), else where it starts.
    pub(super) fn removed_road_label(&self, pts: &[Vec2]) -> String {
        let start = pts.first().copied().unwrap_or_default();
        let mid = match pts {
            [a, b, ..] => a.lerp(*b, 0.5),
            _ => start,
        };
        let name = self.roads.nearest_where(mid, |s| !s.is_rail()).filter(|(_, _, q)| q.distance(mid) < 3.0).and_then(|(sid, _, _)| self.streets.of(sid)).map(|st| st.name.clone());
        match name {
            Some(name) => format!("taking out {name}"),
            None => format!("taking out the road from ({:.0}, {:.0})", start.x, start.y),
        }
    }

    /// A structure the trial rebuild loses: the planned (or removed) road
    /// nearest it.
    pub(super) fn lost_block(&self, id: BuildingId, pv: &PlanPreview) -> Blocker {
        let b = &self.buildings[id];
        let at = self.lots.get(b.lot).map_or(Vec2::ZERO, |l| l.center);
        let nearest = pv
            .planned_roads
            .iter()
            .map(|r| (r, true))
            .chain(pv.removed_roads.iter().map(|r| (r, false)))
            .map(|((gid, pts, ..), new)| (gid, pts, new, closest_on(at, pts)))
            .min_by(|x, y| x.3.distance(at).total_cmp(&y.3.distance(at)));
        let (road, road_label, q) = match nearest {
            Some((gid, pts, true, q)) => (Some(*gid), self.planned_road_label(pts), q),
            Some((gid, pts, false, q)) => (Some(*gid), self.removed_road_label(pts), q),
            None => (None, "the project".to_string(), at),
        };
        Blocker { building: id, name: b.name.clone(), at, road, road_label, a: q, b: q, runs_over: false }
    }

    /// The city's own structures whose lots the asphalt of one road would
    /// cover (`pts` drawn at `levels`, as the road tool and the preview
    /// give them): what the road tools outline in red, and what refuses
    /// implementing. Points on a road already built are skipped (a junction
    /// onto the street in front is no trespass), and so are bored tunnels,
    /// which leave the ground alone.
    pub fn structures_in_way(&self, pts: &[Vec2], levels: &[f32], fwd: u8, bwd: u8, kind: RoadKind) -> Vec<BuildingId> {
        self.structures_in_way_at(pts, levels, fwd, bwd, kind).into_iter().map(|(id, ..)| id).collect()
    }

    /// `structures_in_way`, each with the stretch of the road over it
    /// (the first and last points found on its ground).
    pub fn structures_in_way_at(&self, pts: &[Vec2], levels: &[f32], fwd: u8, bwd: u8, kind: RoadKind) -> Vec<(BuildingId, Vec2, Vec2)> {
        let civic: Vec<(BuildingId, &Lot)> = self
            .buildings
            .iter()
            .filter(|(_, b)| Self::is_civic_structure(b))
            .filter_map(|(id, b)| self.lots.get(b.lot).filter(|l| l.building == Some(id) && l.polygon.len() >= 3).map(|l| (id, l)))
            .collect();
        let mut out: Vec<(BuildingId, Vec2, Vec2)> = Vec::new();
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
                    if l.center.distance(p) > hw + l.polygon.iter().map(|c| c.distance(l.center)).fold(0.0, f32::max) {
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
                        match out.iter_mut().find(|(o, ..)| o == id) {
                            Some(hit) => hit.2 = p,
                            None => out.push((*id, p, p)),
                        }
                    }
                }
            }
        }
        out
    }

    /// Civic structures `project`'s new or changed roads (or roundabouts)
    /// would run over (`structures_in_way` for each).
    pub(super) fn road_blockers(&self, project: &Project) -> Vec<BuildingId> {
        let mut out: Vec<BuildingId> = self.road_blocks(&preview(&self.master, project)).into_iter().map(|k| k.building).collect();
        out.sort();
        out
    }

    /// `road_blockers`, each with the road that takes it and where.
    pub(super) fn road_blocks(&self, pv: &PlanPreview) -> Vec<Blocker> {
        let mut out: Vec<Blocker> = Vec::new();
        let at = |id: BuildingId| self.buildings.get(id).and_then(|b| self.lots.get(b.lot)).map_or(Vec2::ZERO, |l| l.center);
        let name = |id: BuildingId| self.buildings.get(id).map(|b| b.name.clone()).unwrap_or_default();
        for ((gid, pts, fwd, bwd, kind), levels) in pv.planned_roads.iter().zip(&pv.planned_levels) {
            for (id, a, b) in self.structures_in_way_at(pts, levels, *fwd, *bwd, *kind) {
                if !out.iter().any(|k| k.building == id) {
                    out.push(Blocker { building: id, name: name(id), at: at(id), road: Some(*gid), road_label: self.planned_road_label(pts), a, b, runs_over: true });
                }
            }
        }
        let civic = |b: &BuildingId| self.buildings.get(*b).is_some_and(Self::is_civic_structure);
        for (gid, effect) in &pv.planned_rings {
            if let Some(ring) = effect.ring {
                for lot in self.roundabout_takes(ring) {
                    if let Some(id) = self.lots[lot].building.filter(|b| civic(b) && !out.iter().any(|k| k.building == *b)) {
                        let label = format!("the new roundabout at ({:.0}, {:.0})", ring.center.x, ring.center.y);
                        let q = closest_on(ring.center, &self.lots[lot].polygon);
                        out.push(Blocker { building: id, name: name(id), at: at(id), road: Some(*gid), road_label: label, a: q, b: q, runs_over: true });
                    }
                }
            }
        }
        out
    }
}

/// [r5-fixes] The pockets in `after` that were not there in `before` (a
/// pocket's middle is the same however its streets are cut up).
fn new_traps(before: &[crate::roads::Trap], after: &[crate::roads::Trap]) -> Vec<crate::roads::Trap> {
    let old: Vec<Vec2> = before.iter().map(|t| t.at).collect();
    after.iter().filter(|t| !old.iter().any(|o| o.distance(t.at) < 1.0)).cloned().collect()
}
