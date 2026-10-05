use crate::buildings::{Building, BuildingId};
use crate::citizens::{Activity, Citizen, CitizenId, Purpose};
use crate::clock::SimClock;
use crate::economy::Economy;
use crate::events::SimEvent;
use crate::mind::{Decision, Intent, MindContext, MindProvider};
use crate::names;
use crate::plan::{preview, Gesture, GestureId, Plan, PlanPreview, Project};
use crate::roads::{NodeId, RoadNetwork, SegmentId};
use crate::traffic::Traffic;
use crate::zoning::{generate_lots, LandUse, Lot, LotId, Zone, ZoneId};
use crate::SimRng;
use glam::Vec2;
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldConfig {
    pub seed: u64,
    /// Half-size of the buildable square, in metres.
    pub half_extent: f32,
    /// Metres per tick (minute) when walking.
    pub walk_speed: f32,
    /// Trips at least this long (straight line) are driven, shorter ones walked.
    pub drive_threshold: f32,
    /// Seconds of vehicle physics simulated per tick. Smaller than 60 keeps
    /// trips on screen for several sim minutes, as city builders usually do.
    pub traffic_seconds_per_tick: f32,
    pub traffic_substeps: usize,
    pub immigration_interval: u64,
    pub development_interval: u64,
    /// Upper bound on buildings started per development interval.
    pub max_builds_per_interval: usize,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            seed: 7,
            half_extent: 900.0,
            walk_speed: 50.0,
            drive_threshold: 60.0,
            traffic_seconds_per_tick: 10.0,
            traffic_substeps: 10,
            immigration_interval: 30,
            development_interval: 20,
            max_builds_per_interval: 2,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct World {
    pub config: WorldConfig,
    pub clock: SimClock,
    #[serde(with = "rng_serde")]
    rng: SimRng,
    /// What is built.
    pub master: Plan,
    /// Pending changes, if any.
    pub project: Option<Project>,
    next_gesture: u64,
    /// Bumped whenever master or project changes.
    pub plan_version: u64,
    pub roads: RoadNetwork,
    pub zones: SlotMap<ZoneId, Zone>,
    pub lots: SlotMap<LotId, Lot>,
    pub buildings: SlotMap<BuildingId, Building>,
    pub citizens: SlotMap<CitizenId, Citizen>,
    pub traffic: Traffic,
    pub economy: Economy,
    #[serde(skip)]
    pub events: Vec<SimEvent>,
    next_seed: u64,
}

impl World {
    pub fn new(config: WorldConfig) -> Self {
        Self {
            rng: SimRng::seed_from_u64(config.seed),
            config,
            clock: SimClock::starting_at(0, 7, 0),
            master: Plan::default(),
            project: None,
            next_gesture: 1,
            plan_version: 0,
            roads: RoadNetwork::default(),
            zones: SlotMap::with_key(),
            lots: SlotMap::with_key(),
            buildings: SlotMap::with_key(),
            citizens: SlotMap::with_key(),
            traffic: Traffic::default(),
            economy: Economy::default(),
            events: Vec::new(),
            next_seed: 1,
        }
    }

    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default().depth_limit(6))
    }

    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        let mut w: World = ron::from_str(text)?;
        w.after_load();
        Ok(w)
    }

    /// Rebuild derived state that is not serialised.
    fn after_load(&mut self) {
        self.traffic.rebuild(&self.roads);
        for (_, car) in self.traffic.reattach_cars() {
            self.strand_driver(car.citizen, car.pos);
        }
        self.plan_version += 1;
        self.events.push(SimEvent::PlanChanged);
    }

    fn fresh_seed(&mut self) -> u64 {
        self.next_seed = self.next_seed.wrapping_add(1);
        self.next_seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ self.config.seed
    }

    pub fn drain_events(&mut self) -> Vec<SimEvent> {
        std::mem::take(&mut self.events)
    }

    // ── planning ─────────────────────────────────────────────────────────────

    pub fn has_project(&self) -> bool {
        self.project.is_some()
    }

    pub fn start_project(&mut self) {
        if self.project.is_none() {
            self.project = Some(Project::default());
            self.plan_version += 1;
        }
    }

    pub fn discard_project(&mut self) {
        if self.project.take().is_some() {
            self.plan_version += 1;
        }
    }

    fn project_mut(&mut self) -> &mut Project {
        self.start_project();
        self.plan_version += 1;
        self.project.as_mut().unwrap()
    }

    pub fn add_gesture(&mut self, gesture: Gesture) -> GestureId {
        let id = GestureId(self.next_gesture);
        self.next_gesture += 1;
        self.project_mut().push(id, Some(gesture));
        id
    }

    /// Replace a gesture in the project. With `commit == false` the last step
    /// is amended instead of a new one being recorded (continuous drags).
    pub fn set_gesture(&mut self, id: GestureId, gesture: Gesture, commit: bool) {
        let p = self.project_mut();
        if commit {
            p.push(id, Some(gesture));
        } else {
            p.amend(id, Some(gesture));
        }
    }

    pub fn remove_gesture(&mut self, id: GestureId) {
        self.project_mut().push(id, None);
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
            self.plan_version += 1;
        }
        ok
    }

    pub fn redo(&mut self) -> bool {
        let ok = self.project.as_mut().map_or(false, |p| p.redo());
        if ok {
            self.plan_version += 1;
        }
        ok
    }

    pub fn effective_plan(&self) -> Plan {
        match &self.project {
            Some(p) => p.apply_to(&self.master),
            None => self.master.clone(),
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
            self.plan_version += 1;
            return;
        }
        self.master = project.apply_to(&self.master);
        self.rebuild_from_master();
    }

    /// Convenience for scripts and tests: build a road right away.
    pub fn add_road(&mut self, points: &[Vec2], lanes_fwd: u8, lanes_bwd: u8) -> GestureId {
        let had_project = self.project.take();
        let id = self.add_gesture(Gesture::Road { points: points.to_vec(), lanes_fwd, lanes_bwd });
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

    fn rebuild_from_master(&mut self) {
        self.plan_version += 1;

        // Roads.
        let mut roads = RoadNetwork::default();
        for (_, points, fwd, bwd) in self.master.roads() {
            let (fwd, bwd) = if fwd == 0 && bwd == 0 { (1, 1) } else { (fwd, bwd) };
            roads.add_polyline(points, fwd, bwd);
        }
        roads.version = self.roads.version + 1;
        self.roads = roads;

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
        let mut demolish: Vec<BuildingId> = Vec::new();
        for (_, old) in old_lots.iter() {
            let Some(bid) = old.building else { continue };
            let found = self
                .lots
                .iter()
                .filter(|(_, l)| l.building.is_none())
                .map(|(id, l)| (id, l.center.distance(old.center)))
                .filter(|(_, d)| *d < 4.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id);
            match found {
                Some(new_lot) => {
                    self.lots[new_lot].building = Some(bid);
                    if let Some(b) = self.buildings.get_mut(bid) {
                        b.lot = new_lot;
                    }
                }
                None => demolish.push(bid),
            }
        }
        for bid in demolish {
            self.demolish(bid);
        }

        // Lanes and cars.
        self.traffic.rebuild(&self.roads);
        for (_, car) in self.traffic.clear_cars() {
            self.strand_driver(car.citizen, car.pos);
        }

        self.events.push(SimEvent::RoadsChanged);
        self.events.push(SimEvent::LotsChanged);
        self.events.push(SimEvent::PlanChanged);
    }

    fn demolish(&mut self, bid: BuildingId) {
        let Some(b) = self.buildings.remove(bid) else { return };
        let tick = self.clock.tick;
        let center = self.lots.get(b.lot).map(|l| l.center).unwrap_or(Vec2::ZERO);
        for cid in b.residents.iter().chain(b.workers.iter()).copied() {
            if let Some(c) = self.citizens.get_mut(cid) {
                if c.home == Some(bid) {
                    c.home = None;
                    c.remember(tick, format!("{} was demolished. Homeless again.", b.name));
                }
                if c.work == Some(bid) {
                    c.work = None;
                }
            }
        }
        let inside: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.inside == Some(bid)).map(|(id, _)| id).collect();
        for cid in inside {
            let c = &mut self.citizens[cid];
            c.inside = None;
            c.pos = center;
            c.activity = Activity::Idle;
        }
        self.events.push(SimEvent::BuildingRemoved(bid, b.lot));
    }

    /// A driver whose car vanished (network rebuilt, bad save) stands where
    /// the car was and decides again.
    fn strand_driver(&mut self, cid: CitizenId, pos: Vec2) {
        if let Some(c) = self.citizens.get_mut(cid) {
            c.pos = pos;
            c.inside = None;
            c.activity = Activity::Idle;
            c.next_decision_tick = self.clock.tick + 1;
        }
    }

    // ── queries ──────────────────────────────────────────────────────────────

    pub fn building_lot(&self, b: BuildingId) -> Option<&Lot> {
        self.buildings.get(b).and_then(|b| self.lots.get(b.lot))
    }

    pub fn building_entrance(&self, b: BuildingId) -> Option<Vec2> {
        self.building_lot(b).map(|l| l.corners[0].lerp(l.corners[1], 0.5))
    }

    fn available_land_uses(&self) -> [bool; 6] {
        let mut out = [false; 6];
        for b in self.buildings.values() {
            out[b.land_use.index()] = true;
        }
        out
    }

    // ── tick ─────────────────────────────────────────────────────────────────

    pub fn tick(&mut self, minds: &mut dyn MindProvider) {
        self.clock.advance();
        let tick = self.clock.tick;
        if self.clock.minute_of_day() == 0 {
            self.events.push(SimEvent::NewDay(self.clock.day()));
        }
        if tick % 10 == 0 {
            self.update_economy();
        }
        if tick % self.config.development_interval == 0 {
            self.develop();
        }
        if tick % self.config.immigration_interval == 0 {
            self.immigrate();
        }
        if tick % 120 == 0 {
            self.job_search();
        }
        for (id, decision) in minds.poll() {
            if self.citizens.contains_key(id) {
                self.apply_decision(id, decision);
            }
        }
        self.step_traffic();
        self.step_citizens(minds);
    }

    fn step_traffic(&mut self) {
        let arrived = self.traffic.step(self.config.traffic_seconds_per_tick, self.config.traffic_substeps);
        for car_id in arrived {
            let Some(car) = self.traffic.remove_car(car_id) else { continue };
            if let Some(c) = self.citizens.get_mut(car.citizen) {
                c.pos = car.pos;
                c.heading = car.heading;
                self.arrive(car.citizen);
            }
        }
        // Keep citizen positions in sync with their cars for inspection.
        for car in self.traffic.cars.values() {
            if let Some(c) = self.citizens.get_mut(car.citizen) {
                c.pos = car.pos;
                c.heading = car.heading;
            }
        }
    }

    fn update_economy(&mut self) {
        let e = &mut self.economy;
        e.population = self.citizens.len() as u32;
        e.unhoused = self.citizens.values().filter(|c| c.home.is_none()).count() as u32;
        e.unemployed = self.citizens.values().filter(|c| c.work.is_none()).count() as u32;
        e.homes_total = self.buildings.values().map(|b| b.homes as u32).sum();
        e.homes_free = self.buildings.values().map(|b| b.free_homes() as u32).sum();
        e.jobs_total = self.buildings.values().map(|b| b.jobs as u32).sum();
        e.jobs_free = self.buildings.values().map(|b| b.free_jobs() as u32).sum();
        e.recompute();
    }

    fn develop(&mut self) {
        let vacant: Vec<LotId> = self.lots.iter().filter(|(_, l)| l.building.is_none()).map(|(id, _)| id).collect();
        if vacant.is_empty() {
            return;
        }
        let mut built = 0;
        let start = self.rng.random_range(0..vacant.len());
        for i in 0..vacant.len() {
            if built >= self.config.max_builds_per_interval {
                break;
            }
            let lot_id = vacant[(start + i) % vacant.len()];
            let lu = self.lots[lot_id].land_use;
            let chance = match lu {
                LandUse::Park => 0.05,
                _ => self.economy.demand_for(lu) * 0.06,
            };
            if self.rng.random::<f32>() < chance {
                self.build_on(lot_id);
                built += 1;
            }
        }
    }

    pub fn build_on(&mut self, lot_id: LotId) {
        let lu = self.lots[lot_id].land_use;
        let floors = match lu {
            LandUse::Residential => self.rng.random_range(2..=9),
            LandUse::Commercial => self.rng.random_range(2..=12),
            LandUse::Industrial => self.rng.random_range(1..=3),
            LandUse::Entertainment => self.rng.random_range(2..=6),
            LandUse::Civic => self.rng.random_range(2..=5),
            LandUse::Park => 0,
        };
        let seed = self.fresh_seed();
        let name = names::building_name(&mut self.rng, lu);
        let b = Building::new(lot_id, lu, floors, seed, name, self.clock.tick);
        let id = self.buildings.insert(b);
        self.lots[lot_id].building = Some(id);
        self.events.push(SimEvent::BuildingBuilt(id));
    }

    fn entry_node(&mut self) -> Option<NodeId> {
        let leaves = self.roads.leaf_nodes();
        if leaves.is_empty() {
            return None;
        }
        let max = leaves.iter().map(|&n| self.roads.node(n).pos.length()).fold(0.0, f32::max);
        let far: Vec<NodeId> = leaves.iter().copied().filter(|&n| self.roads.node(n).pos.length() >= max * 0.6).collect();
        far.choose(&mut self.rng).copied()
    }

    fn pick_building<F: Fn(&Building) -> bool>(&mut self, pred: F) -> Option<BuildingId> {
        let candidates: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| pred(b)).map(|(id, _)| id).collect();
        candidates.choose(&mut self.rng).copied()
    }

    fn immigrate(&mut self) {
        let free_homes: u32 = self.buildings.values().map(|b| b.free_homes() as u32).sum();
        if free_homes == 0 {
            self.economy.waiting_immigrants = (self.economy.waiting_immigrants + 1).min(60);
            return;
        }
        let Some(entry) = self.entry_node() else { return };
        let n = self.rng.random_range(1..=3).min(free_homes);
        for _ in 0..n {
            let Some(home) = self.pick_building(|b| b.free_homes() > 0) else { break };
            let seed = self.fresh_seed();
            let name = names::citizen_name(&mut self.rng);
            let tick = self.clock.tick;
            let pos = self.roads.node(entry).pos;
            let mut citizen = Citizen {
                name,
                seed,
                home: Some(home),
                work: None,
                pos,
                heading: 0.0,
                activity: Activity::Idle,
                memories: Vec::new(),
                thought: "New city. New me.".to_string(),
                spotlight: false,
                next_decision_tick: tick + self.rng.random_range(5..30),
                arrived_tick: tick,
                inside: None,
                mood: 0.6,
            };
            citizen.remember(tick, format!("Rolled into Neon District on day {} at {}.", self.clock.day() + 1, self.clock.format_hhmm()));
            let id = self.citizens.insert(citizen);
            self.buildings[home].residents.push(id);
            let home_name = self.buildings[home].name.clone();
            self.citizens[id].remember(tick, format!("Moved into {home_name}."));
            if let Some(job) = self.pick_building(|b| b.free_jobs() > 0) {
                self.assign_job(id, job);
            }
            self.economy.waiting_immigrants = self.economy.waiting_immigrants.saturating_sub(1);
            self.travel_to(id, Some(home), Purpose::Home);
            self.events.push(SimEvent::CitizenArrived(id));
        }
    }

    fn assign_job(&mut self, id: CitizenId, job: BuildingId) {
        self.citizens[id].work = Some(job);
        self.buildings[job].workers.push(id);
        let name = self.buildings[job].name.clone();
        let tick = self.clock.tick;
        self.citizens[id].remember(tick, format!("Got hired at {name}."));
    }

    fn job_search(&mut self) {
        let jobless: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.work.is_none()).map(|(id, _)| id).collect();
        for id in jobless {
            let Some(job) = self.pick_building(|b| b.free_jobs() > 0) else { break };
            self.assign_job(id, job);
        }
    }

    fn step_citizens(&mut self, minds: &mut dyn MindProvider) {
        let tick = self.clock.tick;
        let available = self.available_land_uses();
        let population = self.citizens.len() as u32;
        let ids: Vec<CitizenId> = self.citizens.keys().collect();

        for id in ids {
            // Walking.
            let arrived = {
                let c = &mut self.citizens[id];
                if let Activity::Traveling { path, idx, .. } = &mut c.activity {
                    let mut remaining = self.config.walk_speed;
                    while remaining > 0.0 && *idx < path.len() {
                        let target = path[*idx];
                        let d = c.pos.distance(target);
                        if d <= remaining {
                            c.pos = target;
                            *idx += 1;
                            remaining -= d;
                        } else {
                            let dir = (target - c.pos) / d;
                            c.pos += dir * remaining;
                            c.heading = dir.y.atan2(dir.x);
                            remaining = 0.0;
                        }
                    }
                    *idx >= path.len()
                } else {
                    false
                }
            };
            if arrived {
                self.arrive(id);
                continue;
            }

            // Decisions.
            if self.citizens[id].next_decision_tick > tick || self.citizens[id].activity.is_traveling() {
                continue;
            }
            let decision = {
                let c = &self.citizens[id];
                let ctx = MindContext {
                    id,
                    citizen: c,
                    clock: &self.clock,
                    has_home: c.home.is_some(),
                    has_work: c.work.is_some(),
                    home_name: c.home.and_then(|h| self.buildings.get(h)).map(|b| b.name.as_str()),
                    work_name: c.work.and_then(|w| self.buildings.get(w)).map(|b| b.name.as_str()),
                    available,
                    population,
                };
                minds.decide(&ctx)
            };
            match decision {
                Some(d) => {
                    self.citizens[id].next_decision_tick = tick + 20 + self.rng.random_range(0..40);
                    self.apply_decision(id, d);
                }
                None => self.citizens[id].next_decision_tick = tick + 5,
            }
        }
    }

    fn arrive(&mut self, id: CitizenId) {
        let tick = self.clock.tick;
        let (to, purpose) = match &self.citizens[id].activity {
            Activity::Traveling { to, purpose, .. } | Activity::Driving { to, purpose, .. } => (*to, *purpose),
            _ => return,
        };
        let c = &mut self.citizens[id];
        match (purpose, to) {
            (Purpose::Home, Some(b)) => {
                c.activity = Activity::AtHome;
                c.inside = Some(b);
            }
            (Purpose::Work, Some(b)) => {
                c.activity = Activity::Working;
                c.inside = Some(b);
            }
            (Purpose::Visit(_), Some(b)) => {
                c.activity = Activity::Visiting(b);
                c.inside = Some(b);
                if let Some(bld) = self.buildings.get_mut(b) {
                    bld.visitors += 1;
                    let name = bld.name.clone();
                    c.remember(tick, format!("Spent the evening at {name}."));
                    c.mood = (c.mood + 0.05).min(1.0);
                }
            }
            _ => {
                c.activity = Activity::Idle;
                c.inside = None;
            }
        }
        c.next_decision_tick = tick + 30 + self.rng.random_range(0..90);
    }

    fn apply_decision(&mut self, id: CitizenId, d: Decision) {
        let tick = self.clock.tick;
        {
            let c = &mut self.citizens[id];
            if !d.thought.is_empty() && d.thought != c.thought {
                c.thought = d.thought.clone();
                self.events.push(SimEvent::NewThought(id));
            }
            if let Some(m) = d.memory {
                c.remember(tick, m);
            }
        }
        let (home, work, inside) = {
            let c = &self.citizens[id];
            (c.home, c.work, c.inside)
        };
        match d.intent {
            Intent::Stay => {}
            Intent::GoHome => {
                if let Some(h) = home {
                    if inside != Some(h) {
                        self.travel_to(id, Some(h), Purpose::Home);
                    }
                }
            }
            Intent::GoWork => {
                if let Some(w) = work {
                    if inside != Some(w) {
                        self.travel_to(id, Some(w), Purpose::Work);
                    }
                }
            }
            Intent::Visit(lu) => {
                let target = self.pick_building(|b| b.land_use == lu);
                if let Some(b) = target {
                    if inside != Some(b) {
                        self.travel_to(id, Some(b), Purpose::Visit(lu));
                    }
                }
            }
            Intent::Wander => self.wander(id),
        }
    }

    fn leave_building(&mut self, id: CitizenId) -> Option<Vec2> {
        let c = &mut self.citizens[id];
        let inside = c.inside.take()?;
        if matches!(c.activity, Activity::Visiting(_)) {
            if let Some(b) = self.buildings.get_mut(inside) {
                b.visitors = b.visitors.saturating_sub(1);
            }
        }
        self.building_entrance(inside)
    }

    /// Where a citizen currently touches the road network: segment, point, node.
    fn origin_for(&self, id: CitizenId) -> Option<(SegmentId, Vec2, NodeId)> {
        let c = &self.citizens[id];
        if let Some(b) = c.inside {
            let lot = self.building_lot(b)?;
            let entrance = lot.corners[0].lerp(lot.corners[1], 0.5);
            return Some((lot.front_segment, entrance, lot.access_node));
        }
        let (sid, t, q) = self.roads.nearest_on_network(c.pos)?;
        Some((sid, q, self.roads.nearest_endpoint(sid, t)))
    }

    fn travel_to(&mut self, id: CitizenId, target: Option<BuildingId>, purpose: Purpose) {
        let Some(target) = target else { return };
        let Some((from_seg, from_pt, from_node)) = self.origin_for(id) else {
            self.citizens[id].activity = Activity::Idle;
            return;
        };
        let Some(lot) = self.building_lot(target) else { return };
        let (to_seg, to_node) = (lot.front_segment, lot.access_node);
        let entrance = lot.corners[0].lerp(lot.corners[1], 0.5);

        // Drive when it is far enough and a lane route exists.
        if from_pt.distance(entrance) >= self.config.drive_threshold {
            if let Some((route, s0, s1)) = self.traffic.net.find_route(from_seg, from_pt, to_seg, entrance) {
                let exit_pos = self.leave_building(id);
                let color = self.rng.random_range(0..8u8);
                let car = self.traffic.spawn_car(id, route, s0, s1, color);
                let c = &mut self.citizens[id];
                if let Some(p) = exit_pos {
                    c.pos = p;
                }
                c.activity = Activity::Driving { car, to: Some(target), purpose };
                return;
            }
        }

        // Otherwise walk along the node graph.
        let Some(nodes) = self.roads.path(from_node, to_node) else {
            let c = &mut self.citizens[id];
            c.thought = "No way through. The grid doesn't connect.".to_string();
            c.activity = Activity::Idle;
            return;
        };
        let exit_pos = self.leave_building(id);
        let c = &mut self.citizens[id];
        if let Some(p) = exit_pos {
            c.pos = p;
        }
        let mut path: Vec<Vec2> = nodes.iter().map(|&n| self.roads.node(n).pos).collect();
        path.push(entrance);
        c.activity = Activity::Traveling { to: Some(target), purpose, path, idx: 0 };
    }

    fn wander(&mut self, id: CitizenId) {
        let Some((_, _, start)) = self.origin_for(id) else { return };
        if self.roads.nodes.is_empty() {
            return;
        }
        let end = NodeId(self.rng.random_range(0..self.roads.nodes.len() as u32));
        let Some(nodes) = self.roads.path(start, end) else { return };
        let exit_pos = self.leave_building(id);
        let c = &mut self.citizens[id];
        if let Some(p) = exit_pos {
            c.pos = p;
        }
        let path: Vec<Vec2> = nodes.iter().map(|&n| self.roads.node(n).pos).collect();
        c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
    }
}

/// RON cannot encode the RNG's 128-bit counter, so persist it as seed + position.
mod rng_serde {
    use crate::SimRng;
    use rand::SeedableRng;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Saved {
        seed: [u8; 32],
        stream: u64,
        pos_hi: u64,
        pos_lo: u64,
    }

    pub fn serialize<S: Serializer>(rng: &SimRng, s: S) -> Result<S::Ok, S::Error> {
        let pos = rng.get_word_pos();
        Saved { seed: rng.get_seed(), stream: rng.get_stream(), pos_hi: (pos >> 64) as u64, pos_lo: pos as u64 }.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SimRng, D::Error> {
        let saved = Saved::deserialize(d)?;
        let mut rng = SimRng::from_seed(saved.seed);
        rng.set_stream(saved.stream);
        rng.set_word_pos(((saved.pos_hi as u128) << 64) | saved.pos_lo as u128);
        Ok(rng)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::IdleMind;

    fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
    }

    #[test]
    fn city_grows_and_round_trips_through_ron() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(0.0, -400.0), Vec2::new(0.0, 400.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
        assert!(!w.lots.is_empty());
        let mut minds = IdleMind;
        for _ in 0..(24 * 60) {
            w.tick(&mut minds);
        }
        assert!(!w.buildings.is_empty());
        assert!(!w.citizens.is_empty());
        let text = w.to_ron().unwrap();
        let back = World::from_ron(&text).unwrap();
        assert_eq!(back.citizens.len(), w.citizens.len());
        assert_eq!(back.traffic.net.lanes.len(), w.traffic.net.lanes.len());
    }

    #[test]
    fn projects_undo_redo_and_keep_buildings_on_implement() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        let lot = w.lots.keys().next().unwrap();
        w.build_on(lot);
        let building = w.lots[lot].building.unwrap();

        let id = w.add_gesture(Gesture::Road { points: vec![Vec2::new(300.0, -300.0), Vec2::new(300.0, 300.0)], lanes_fwd: 1, lanes_bwd: 1 });
        assert_eq!(w.preview().planned_roads.len(), 1);
        assert!(w.undo());
        assert_eq!(w.preview().planned_roads.len(), 0);
        assert!(w.redo());
        w.set_gesture(id, Gesture::Road { points: vec![Vec2::new(320.0, -300.0), Vec2::new(320.0, 300.0)], lanes_fwd: 2, lanes_bwd: 2 }, true);
        w.implement();
        assert!(!w.has_project());
        assert_eq!(w.master.gestures.len(), 3);
        assert!(w.buildings.contains_key(building), "building on an untouched lot survives a rebuild");
        assert!(w.roads.alive_segments().count() >= 3);
        assert!(!w.traffic.net.intersections.is_empty());
    }

    #[test]
    fn newest_zone_wins_and_repaint_changes_lots() {
        use crate::zoning::point_in_polygon;
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        let tri = vec![Vec2::new(-180.0, 10.0), Vec2::new(250.0, 10.0), Vec2::new(-180.0, 250.0)];
        let first = w.add_zone(tri.clone(), LandUse::Entertainment).unwrap();
        assert!(w.lots.values().all(|l| l.land_use == LandUse::Entertainment));
        // Drawing a commercial zone over it repaints the ground.
        w.add_zone(tri.clone(), LandUse::Commercial);
        assert!(w.lots.values().filter(|l| point_in_polygon(l.center, &tri)).all(|l| l.land_use == LandUse::Commercial));
        // Repainting the original gesture and implementing also works.
        assert!(w.repaint_zone(first, LandUse::Park));
        w.implement();
        // The newer commercial zone still sits on top of the repainted one.
        assert!(w.lots.values().filter(|l| point_in_polygon(l.center, &tri)).all(|l| l.land_use == LandUse::Commercial));
    }

    #[test]
    fn citizens_drive_between_buildings() {
        let mut w = World::new(WorldConfig::default());
        for i in -1..=1 {
            let c = i as f32 * 200.0;
            w.add_road(&[Vec2::new(-500.0, c), Vec2::new(500.0, c)], 1, 1);
            w.add_road(&[Vec2::new(c, -500.0), Vec2::new(c, 500.0)], 1, 1);
        }
        w.add_zone(square(-190.0, -190.0, 180.0), LandUse::Residential);
        w.add_zone(square(10.0, 10.0, 180.0), LandUse::Commercial);
        let mut minds = IdleMind;
        let mut saw_car = false;
        for _ in 0..(2 * 24 * 60) {
            w.tick(&mut minds);
            if !w.traffic.cars.is_empty() {
                saw_car = true;
            }
        }
        assert!(saw_car, "commuters should have driven at some point");
    }
}
