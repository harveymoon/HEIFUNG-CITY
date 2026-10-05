//! Planning: projects and gestures, undo/redo, the preview, and
//! `rebuild_from_master`, which turns the master plan into roads, zones,
//! lots and buildings (eminent domain, lot matching, demolition).

use super::*;

impl World {
    /// Convenience for scripts and tests: lay track right away.
    pub fn add_rail(&mut self, points: &[Vec2]) -> GestureId {
        let had_project = self.project.take();
        let id = self.add_gesture(Gesture::Road { points: points.to_vec(), lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: RoadKind::Rail });
        self.implement();
        self.project = had_project;
        id
    }

    pub fn has_project(&self) -> bool {
        self.project.is_some()
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
    }

    pub fn start_project(&mut self) {
        if self.project.is_none() {
            self.project = Some(Project::default());
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
    pub fn implement(&mut self) {
        let Some(project) = self.project.take() else { return };
        if project.is_empty() {
            self.plan_changed();
            return;
        }
        self.master = project.apply_to(&self.master);
        self.rebuild_from_master();
    }

    /// Convenience for scripts and tests: build a road right away.
    pub fn add_road(&mut self, points: &[Vec2], lanes_fwd: u8, lanes_bwd: u8) -> GestureId {
        let had_project = self.project.take();
        let id = self.add_gesture(Gesture::Road { points: points.to_vec(), lanes_fwd, lanes_bwd, curved: false, kind: RoadKind::Street });
        self.implement();
        self.project = had_project;
        id
    }

    /// Convenience for scripts and tests: a walking and cycling path right away.
    pub fn add_path(&mut self, points: &[Vec2]) -> GestureId {
        let had_project = self.project.take();
        let id = self.add_gesture(Gesture::Road { points: points.to_vec(), lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: RoadKind::Path });
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
        for (_, points, fwd, bwd, kind) in self.master.roads() {
            // Track is eased into smooth curves and tangential switches.
            let points = if kind == RoadKind::Rail { rail::smooth_track(&points, &roads) } else { points };
            let (fwd, bwd) = match kind {
                RoadKind::Street if fwd == 0 && bwd == 0 => (1, 1),
                RoadKind::Street => (fwd, bwd),
                RoadKind::Path | RoadKind::Rail => (0, 0),
            };
            roads.add_polyline_kind(&points, fwd, bwd, kind);
        }
        roads.version = self.roads.version + 1;
        self.roads = roads;
        self.rebuild_road_components();

        // Zones, newest first so a zone drawn over an older one takes the ground.
        // SlotMap reuses freed slots, so iteration order is not insertion order;
        // keep the ids in the order we need them.
        self.zones.clear();
        let zones: Vec<_> = self.master.zones().collect();
        let zone_ids: Vec<ZoneId> = zones
            .into_iter()
            .rev()
            .map(|(_, polygon, land_use)| self.zones.insert(Zone { polygon: polygon.to_vec(), land_use }))
            .collect();

        // Lots: regenerate, then match old lots with buildings to new ones.
        let old_lots = std::mem::replace(&mut self.lots, SlotMap::with_key());
        let mut covering: Vec<Zone> = Vec::new();
        for zid in zone_ids {
            let zone = self.zones[zid].clone();
            generate_lots(zid, &zone, &self.roads, &mut self.lots, &covering);
            covering.push(zone);
        }
        // Facilities: their footprints are lots of their own, and zoned lots
        // under them give way.
        // A lot whose centre is under a footprint, or whose outline crosses
        // one, would share ground with the station or yard.
        let footprints: Vec<Vec<Vec2>> = self.facilities.values().map(|f| f.footprint()).collect();
        self.lots.retain(|_, l| !footprints.iter().any(|fp| point_in_polygon(l.center, fp) || polygons_overlap(&shrink_about_centroid(fp, 0.98), &l.polygon)));
        let fids: Vec<FacilityId> = self.facilities.keys().collect();
        for fid in fids {
            // A facility with no street anywhere to front gets no lot this
            // time round; its building goes with the old lot below.
            self.facilities[fid].lot = self.facility_lot(fid).map(|lot| self.lots.insert(lot));
        }
        // Match old lots to new ones through a coarse grid of the new lots'
        // bounding boxes, so a rebuild costs lots × neighbours, not lots².
        let grid = LotGrid::new(&self.lots);
        // (building, where it stood, pulled down for the works?)
        let mut demolish: Vec<(BuildingId, Vec2, bool)> = Vec::new();
        for (_, old) in old_lots.iter() {
            let Some(bid) = old.building else { continue };
            let found = grid
                .near(old.center, 6.0)
                .into_iter()
                .map(|id| (id, &self.lots[id]))
                .filter(|(_, l)| l.building.is_none() && l.usable())
                .map(|(id, l)| (id, if l.contains(old.center) { 0.0 } else { l.center.distance(old.center) }))
                .filter(|(_, d)| *d < 6.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id);
            match found {
                // Eminent domain: a parcel reshaped by road or path works
                // cannot keep its building, even if a lot is still there.
                Some(new_lot) if outlines_differ(&old.polygon, &self.lots[new_lot].polygon, EXPROPRIATE_AREA_TOL, EXPROPRIATE_DIST_TOL) => {
                    demolish.push((bid, old.center, true));
                }
                Some(new_lot) => {
                    self.lots[new_lot].building = Some(bid);
                    if let Some(b) = self.buildings.get_mut(bid) {
                        b.lot = new_lot;
                    }
                }
                None => demolish.push((bid, old.center, false)),
            }
        }
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

        // Lanes and cars.
        self.traffic.rebuild(&self.roads);
        for (_, car) in self.traffic.clear_cars() {
            self.strand_car(car);
        }
        // Trains ran on track that may be gone or re-laid, and their stops
        // and node ids are stale either way: everyone gets off where the
        // train stands and the next headway brings fresh ones.
        self.clear_trains();
        // Bus stops follow the streets they sat on, or go.
        for route in self.transit.routes.values_mut() {
            route.stops = route.stops.iter().filter_map(|s| BusStop::snap(&self.roads, s.pos)).collect();
        }
        self.transit.refresh_legs(&self.traffic.net);
        self.refresh_rail_links();
        self.rebuild_walk();

        self.events.push(SimEvent::RoadsChanged);
        self.events.push(SimEvent::LotsChanged);
        self.events.push(SimEvent::PlanChanged);
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
    /// and decide again.
    pub(super) fn clear_trains(&mut self) {
        let trains: Vec<Train> = self.rail.trains.drain().map(|(_, t)| t).collect();
        for t in trains {
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
    pub(super) fn is_civic_structure(b: &Building) -> bool {
        b.plant.is_some() || b.depot || b.rail.is_some()
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
        let Some(b) = self.buildings.remove(bid) else { return };
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
    }

    /// Someone loses their job: back to gig money, with a note if there is
    /// a story to it.
    pub(super) fn lay_off(&mut self, cid: CitizenId, memory: Option<String>) {
        let tick = self.clock.tick;
        if let Some(c) = self.citizens.get_mut(cid) {
            c.work = None;
            c.income = GIG_INCOME;
            if let Some(m) = memory {
                c.remember(tick, m);
            }
        }
    }

    /// Personal skill factor, fixed per person, spreading incomes within a job.
    pub(super) fn skill_of(seed: u64) -> f32 {
        0.75 + (seed % 1000) as f32 / 1000.0 * 0.6
    }

    /// A car taken off the road: its driver, or a bus's riders, stand there.
    pub(super) fn strand_car(&mut self, car: crate::traffic::Car) {
        // A truck taken off the road still delivers: the buyer already counts
        // the load as inbound and would never reorder it otherwise.
        if let Some(cargo) = car.cargo {
            self.deliver(cargo);
            return;
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
fn shrink_about_centroid(poly: &[Vec2], factor: f32) -> Vec<Vec2> {
    let c = crate::zoning::polygon_centroid(poly);
    poly.iter().map(|&p| c + (p - c) * factor).collect()
}
