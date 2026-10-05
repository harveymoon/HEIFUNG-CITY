use crate::buildings::{Building, BuildingId, Tier};
use crate::citizens::{Activity, Citizen, CitizenId, Purpose};
use crate::clock::SimClock;
use crate::economy::Economy;
use crate::events::SimEvent;
use crate::land_value::{LandValue, LandValueBuilder};
use crate::market::{best_seller, Business, BusinessKind, Resource, EXPORT_DISCOUNT, IMPORT_MARKUP};
use crate::traffic::Cargo;
use crate::mind::{Decision, Intent, MindContext, MindProvider};
use crate::names;
use crate::plan::{preview, Gesture, GestureId, Plan, PlanPreview, Project};
use crate::roads::{NodeId, RoadNetwork, SegmentId};
use crate::traffic::Traffic;
use crate::walk::WalkGraph;
use crate::power::{demand_mw, PowerPlantKind, Weather, GRID_IMPORT_MW, GRID_IMPORT_PRICE};
use crate::traffic::{Bus, CarId};
use std::collections::HashMap;
use crate::transit::{self, BusStop, RouteId, Transit, TransitLeg, TransitTrip};
use crate::rail::{self, Facility, FacilityId, Rail, RailFacility, RailLink, Train, TrainId, TrainKind};
use crate::zoning::point_in_polygon;
use crate::citizens::TravelMode;
use crate::roads::RoadKind;
use crate::zoning::{generate_lots, LandUse, Lot, LotId, Zone, ZoneId};
use crate::SimRng;
use glam::Vec2;
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

pub use crate::rail::TRUNK_Y;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldConfig {
    pub seed: u64,
    /// Half-size of the buildable square, in metres.
    pub half_extent: f32,
    /// Metres per tick (minute) when walking.
    /// Metres walked per tick. Movement shows `traffic_seconds_per_tick` of
    /// real motion per sim minute, so this is 1.4 m/s times that.
    pub walk_speed: f32,
    #[serde(default = "default_bike_speed")]
    pub bike_speed: f32,
    /// Trips at least this long are cycled when the citizen owns a bike.
    #[serde(default = "default_bike_threshold")]
    pub bike_threshold: f32,
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

fn default_bike_speed() -> f32 {
    4.5 * 1.5
}

fn default_bike_threshold() -> f32 {
    40.0
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            seed: 7,
            half_extent: 900.0,
            walk_speed: 1.4 * 1.5,
            bike_speed: default_bike_speed(),
            bike_threshold: default_bike_threshold(),
            drive_threshold: 60.0,
            traffic_seconds_per_tick: 1.5,
            traffic_substeps: 3,
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
    /// Pedestrian network, derived from roads and lots.
    #[serde(skip)]
    pub walk: WalkGraph,
    #[serde(default)]
    pub weather: Weather,
    #[serde(default)]
    pub transit: Transit,
    #[serde(default)]
    pub rail: Rail,
    /// Stations and yards placed as pieces, with their own lots.
    #[serde(default)]
    pub facilities: SlotMap<FacilityId, Facility>,
    pub economy: Economy,
    pub land_value: LandValue,
    #[serde(skip)]
    pub events: Vec<SimEvent>,
    next_seed: u64,
}

impl World {
    pub fn new(config: WorldConfig) -> Self {
        let land_value = LandValue::new(config.half_extent + 200.0, 32.0);
        let mut economy = Economy::default();
        economy.treasury = 60_000.0;
        let half = config.half_extent;
        let mut w = Self {
            land_value,
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
            walk: WalkGraph::default(),
            weather: Weather::default(),
            transit: Transit::default(),
            rail: Rail::default(),
            facilities: SlotMap::with_key(),
            economy,
            events: Vec::new(),
            next_seed: 1,
        };
        // The trunk line: a railway already crosses the map south of the
        // centre. Branches, stations and yards hang off it.
        w.add_rail(&[Vec2::new(-half - 100.0, TRUNK_Y), Vec2::new(half + 100.0, TRUNK_Y)]);
        w
    }

    /// Convenience for scripts and tests: lay track right away.
    pub fn add_rail(&mut self, points: &[Vec2]) -> GestureId {
        let had_project = self.project.take();
        let id = self.add_gesture(Gesture::Road { points: points.to_vec(), lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: RoadKind::Rail });
        self.implement();
        self.project = had_project;
        id
    }

    fn rebuild_walk(&mut self) {
        crate::park::plan_parks(&mut self.lots, &self.land_value);
        self.walk = WalkGraph::build(&self.roads, &self.lots, &self.land_value, self.walk.version + 1);
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
        self.rebuild_walk();
        for (_, car) in self.traffic.reattach_cars() {
            self.strand_car(car);
        }
        self.transit.refresh_legs(&self.traffic.net);
        let on_trains: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::OnTrain { .. })).map(|(id, _)| id).collect();
        for id in on_trains {
            let pos = self.citizens[id].pos;
            self.citizens[id].transit = None;
            self.strand_driver(id, pos);
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

    fn rebuild_from_master(&mut self) {
        self.plan_version += 1;

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
        let footprints: Vec<Vec<Vec2>> = self.facilities.values().map(|f| f.footprint()).collect();
        self.lots.retain(|_, l| !footprints.iter().any(|fp| point_in_polygon(l.center, fp)));
        let fids: Vec<FacilityId> = self.facilities.keys().collect();
        for fid in fids {
            let lot = self.facility_lot(fid);
            let id = self.lots.insert(lot);
            self.facilities[fid].lot = Some(id);
        }
        let mut demolish: Vec<BuildingId> = Vec::new();
        for (_, old) in old_lots.iter() {
            let Some(bid) = old.building else { continue };
            let found = self
                .lots
                .iter()
                .filter(|(_, l)| l.building.is_none() && l.usable())
                .map(|(id, l)| (id, if l.contains(old.center) { 0.0 } else { l.center.distance(old.center) }))
                .filter(|(_, d)| *d < 6.0)
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
            self.strand_car(car);
        }
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
                    c.income = 40.0;
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
    /// A car taken off the road: its driver, or a bus's riders, stand there.
    fn strand_car(&mut self, car: crate::traffic::Car) {
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
        self.building_lot(b).map(|l| l.entrance())
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
            self.daily_update();
        }
        if tick % 60 == 0 {
            self.hourly_economy();
        }
        if tick % 120 == 0 {
            self.update_land_value();
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
        self.step_transit();
        self.step_rail();
        self.step_citizens(minds);
        // Every system that moves people has run: lay the breadcrumbs.
        for c in self.citizens.values_mut() {
            c.push_trail();
        }
    }

    fn step_traffic(&mut self) {
        let arrived = self.traffic.step(self.config.traffic_seconds_per_tick, self.config.traffic_substeps);
        for car_id in arrived {
            if self.traffic.cars.get(car_id).is_some_and(|c| c.bus.is_some()) {
                self.bus_arrived(car_id);
                continue;
            }
            let Some(car) = self.traffic.remove_car(car_id) else { continue };
            if let Some(cargo) = car.cargo {
                self.deliver(cargo);
                continue;
            }
            if let Some(c) = self.citizens.get_mut(car.citizen) {
                c.pos = car.pos;
                c.heading = car.heading;
                self.park_car(car.citizen);
                if !self.walk_last_leg(car.citizen) {
                    self.arrive(car.citizen);
                }
            }
        }
        // Keep citizen positions in sync with their cars for inspection.
        for car in self.traffic.cars.values() {
            if let Some(c) = self.citizens.get_mut(car.citizen) {
                c.pos = car.pos;
                c.heading = car.heading;
            }
            if let Some(bus) = &car.bus {
                for &p in &bus.passengers {
                    if let Some(c) = self.citizens.get_mut(p) {
                        c.pos = car.pos;
                        c.prev_pos = car.pos;
                        c.heading = car.heading;
                    }
                }
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
        e.parking_capacity = self.buildings.values().map(|b| b.parking_capacity as u32).sum();
        e.parking_used = self.buildings.values().map(|b| b.parking_used as u32).sum();
        e.parking_lots = self.buildings.values().filter(|b| b.is_parking()).count() as u32;
        let mut tier_counts = [0u32; 3];
        let mut rent_sum = [0.0f32; 3];
        for b in self.buildings.values().filter(|b| b.land_use == LandUse::Residential) {
            tier_counts[b.tier.index()] += 1;
            rent_sum[b.tier.index()] += b.rent;
        }
        e.tier_counts = tier_counts;
        for i in 0..3 {
            e.avg_rent[i] = if tier_counts[i] > 0 { rent_sum[i] / tier_counts[i] as f32 } else { 0.0 };
        }
        let mut class_counts = [0u32; 3];
        let mut incomes: Vec<f32> = Vec::with_capacity(self.citizens.len());
        for c in self.citizens.values() {
            class_counts[c.class().index()] += 1;
            incomes.push(c.income);
        }
        e.class_counts = class_counts;
        e.gini = Economy::gini(&mut incomes);
        e.avg_land_value = self.land_value.average();
        e.recompute();
    }

    /// Sum accessibility, amenities, nuisances, traffic and neighbours into the
    /// land value grid, then refresh the rent of every building.
    fn update_land_value(&mut self) {
        let mut b = LandValueBuilder::new(&self.land_value);
        for (sid, seg) in self.roads.alive_segments() {
            let (a, bp) = self.roads.segment_points(sid);
            let len = a.distance(bp);
            let steps = (len / 24.0).ceil().max(1.0) as usize;
            let w = if seg.lanes() >= 4 { 0.04 } else { 0.025 };
            for i in 0..=steps {
                b.splat(a.lerp(bp, i as f32 / steps as f32), 110.0, w);
            }
        }
        for bld in self.buildings.values() {
            let Some(lot) = self.lots.get(bld.lot) else { continue };
            let c = lot.center;
            match bld.land_use {
                LandUse::Park => b.splat(c, 180.0, 0.35),
                LandUse::Civic => b.splat(c, 200.0, 0.2),
                LandUse::Commercial => b.splat(c, 120.0, 0.08),
                LandUse::Entertainment => {
                    b.splat(c, 110.0, 0.06);
                    b.splat(c, 50.0, -0.08);
                }
                LandUse::Industrial => b.splat(c, 220.0, -0.3),
                LandUse::Residential => match bld.tier {
                    Tier::High => b.splat(c, 130.0, 0.12),
                    Tier::Low => b.splat(c, 130.0, -0.07),
                    Tier::Mid => {}
                },
            }
            if bld.is_derelict() {
                b.splat(c, 100.0, -0.12);
            }
            if let Some(kind) = bld.plant {
                let (r, w) = kind.pollution();
                if r > 0.0 {
                    b.splat(c, r, w);
                }
            }
            if let Some(link) = &bld.rail {
                match link.kind {
                    RailFacility::Station => b.splat(c, 220.0, 0.25),
                    RailFacility::FreightYard => b.splat(c, 160.0, -0.12),
                }
            }
        }
        for lot in self.lots.values() {
            if lot.building.is_none() {
                b.splat(lot.center, 60.0, -0.03);
            }
        }
        for car in self.traffic.cars.values() {
            b.splat(car.pos, 45.0, -0.004);
        }
        b.finish(0.25, &mut self.land_value);
        let lv = &self.land_value;
        for bld in self.buildings.values_mut() {
            if let Some(lot) = self.lots.get(bld.lot) {
                bld.update_rent(lv.at(lot.center));
            }
        }
    }

    /// Once a sim day: buildings wear, upgrade or decay; citizens pay rent and
    /// react to it.
    fn daily_update(&mut self) {
        // Treasury: yesterday's taxes in, plant upkeep and imported power out.
        let upkeep: f32 = self.buildings.values().filter_map(|b| b.plant).map(|k| k.upkeep_per_day()).sum();
        self.economy.treasury += self.economy.tax_today - upkeep - self.economy.power.import_cost_today;
        self.economy.power.upkeep_today = upkeep;
        self.economy.tax_today = 0.0;
        self.economy.power.import_cost_today = 0.0;
        self.economy.power.brownout_hours_today = 0;
        for b in self.buildings.values_mut() {
            b.unpowered_hours_today = 0;
        }
        // Buses and depots cost money; yesterday's fares were taken as they came.
        let bus_days = self.traffic.cars.values().filter(|c| c.bus.is_some()).count() as f32;
        let depots = self.buildings.values().filter(|b| b.depot).count() as f32;
        let transit_upkeep = bus_days * transit::BUS_UPKEEP_PER_DAY + depots * transit::DEPOT_UPKEEP_PER_DAY;
        self.economy.treasury -= transit_upkeep;
        self.economy.transit.upkeep_today = transit_upkeep;
        let stations = self.buildings.values().filter(|b| b.rail.as_ref().is_some_and(|l| l.kind == RailFacility::Station)).count() as f32;
        let yards = self.buildings.values().filter(|b| b.rail.as_ref().is_some_and(|l| l.kind == RailFacility::FreightYard)).count() as f32;
        let rail_upkeep = stations * rail::STATION_UPKEEP_PER_DAY + yards * rail::YARD_UPKEEP_PER_DAY;
        self.economy.treasury -= rail_upkeep;
        let rs = &mut self.rail.stats;
        rs.upkeep_today = rail_upkeep;
        rs.riders_today = 0;
        rs.fares_today = 0.0;
        rs.passenger_trains_today = 0;
        rs.freight_trains_today = 0;
        rs.freight_in_today = 0.0;
        self.economy.transit.riders_today = 0;
        self.economy.transit.fares_today = 0.0;
        for r in self.transit.routes.values_mut() {
            r.riders_today = 0;
        }
        self.economy.parking_street_today = 0;
        self.economy.parking_fees_today = 0.0;
        self.economy.walk_trips_today = 0;
        self.economy.walk_in_sales_today = 0.0;
        for l in self.lots.values_mut() {
            l.footfall_today = 0;
        }
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        for bid in ids {
            let (lu, tier, value, derelict_before, full) = {
                let b = &self.buildings[bid];
                let full = if b.land_use == LandUse::Residential { b.homes > 0 && b.free_homes() == 0 } else { b.jobs > 0 && b.free_jobs() == 0 };
                (b.land_use, b.tier, b.land_value, b.is_derelict(), full)
            };
            let decay = match tier {
                Tier::Low => 0.02,
                Tier::Mid => 0.008,
                Tier::High => 0.002,
            };
            {
                let b = &mut self.buildings[bid];
                b.condition = (b.condition - decay + if full { 0.006 } else { 0.0 }).clamp(0.05, 1.0);
            }
            let target = Tier::for_land_value(value);
            let mut changed = false;
            if lu != LandUse::Park {
                if target > tier && full && self.rng.random::<f32>() < 0.15 {
                    let b = &mut self.buildings[bid];
                    b.tier = tier.up();
                    b.condition = 1.0;
                    changed = true;
                } else if target < tier && self.rng.random::<f32>() < 0.06 {
                    let b = &mut self.buildings[bid];
                    b.tier = tier.down();
                    b.condition = (b.condition * 0.8).max(0.2);
                    changed = true;
                }
            }
            let derelict_after = self.buildings[bid].is_derelict();
            if changed || derelict_before != derelict_after {
                let seed = self.fresh_seed();
                let b = &mut self.buildings[bid];
                b.seed = seed;
                b.recompute_capacity();
                b.update_rent(value);
                self.events.push(SimEvent::BuildingChanged(bid));
            }
        }
        let cids: Vec<CitizenId> = self.citizens.keys().collect();
        for cid in cids {
            self.daily_citizen(cid);
        }
        self.daily_businesses();
    }

    /// Businesses pay wages; those that cannot for several days close, and
    /// closed buildings are demolished after a week so the lot can be reused.
    fn daily_businesses(&mut self) {
        let tick = self.clock.tick;
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        let mut demolish = Vec::new();
        for bid in ids {
            let revenue = self.buildings[bid].business.as_ref().map_or(0.0, |z| z.revenue_today);
            self.economy.tax_today += revenue * 0.05;
            let (wage_bill, workers, publicly_funded, kind) = {
                let b = &self.buildings[bid];
                let Some(bz) = &b.business else { continue };
                let publicly_funded = matches!(bz.kind, BusinessKind::Office) || b.land_use == LandUse::Civic;
                (b.workers.len() as f32 * b.wage() * 0.6, b.workers.clone(), publicly_funded, bz.kind)
            };
            let b = &mut self.buildings[bid];
            let bz = b.business.as_mut().unwrap();
            if bz.closed {
                bz.broke_days = bz.broke_days.saturating_add(1);
                if bz.broke_days > 7 {
                    demolish.push(bid);
                }
                continue;
            }
            if !publicly_funded {
                bz.money -= wage_bill;
                bz.spend_today += wage_bill;
            }
            bz.revenue_today = 0.0;
            bz.spend_today = 0.0;
            if bz.money < -300.0 && !publicly_funded {
                bz.broke_days = bz.broke_days.saturating_add(1);
            } else {
                bz.broke_days = 0;
            }
            if bz.broke_days >= 5 {
                bz.closed = true;
                bz.broke_days = 0;
                b.condition = b.condition.min(0.25);
                b.workers.clear();
                let name = b.name.clone();
                let _ = kind;
                for w in workers {
                    if let Some(c) = self.citizens.get_mut(w) {
                        c.work = None;
                        c.income = 40.0;
                        c.remember(tick, format!("{name} shut down. Out of a job."));
                        c.mood = (c.mood - 0.15).max(0.0);
                    }
                }
                self.economy.market.closed_total += 1;
                self.events.push(SimEvent::BusinessClosed(bid));
                self.events.push(SimEvent::BuildingChanged(bid));
            }
        }
        for bid in demolish {
            self.demolish(bid);
        }
        self.economy.market.deals_today = 0;
        self.economy.market.imports_today = 0.0;
        self.economy.market.exports_today = 0.0;
        self.economy.market.meals_sold_today = 0;
    }

    /// Every sim hour: hunger, eating at home, production, restocking through
    /// the market or imports, exporting surplus, and market statistics.
    fn hourly_economy(&mut self) {
        let tick = self.clock.tick;
        self.weather.step(&mut self.rng);
        self.allocate_power();
        // Hunger and home cooking.
        for c in self.citizens.values_mut() {
            c.satiety = (c.satiety - 0.04).max(0.0);
            let at_home = c.home.is_some() && c.inside == c.home;
            if at_home && c.satiety < 0.6 && c.groceries >= 1.0 {
                c.groceries -= 1.0;
                c.satiety = (c.satiety + 0.45).min(1.0);
            }
        }
        // Production.
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        for &bid in &ids {
            let b = &mut self.buildings[bid];
            let (jobs, staffing) = (b.jobs, b.staffing());
            if let Some(bz) = b.business.as_mut() {
                if !bz.closed {
                    bz.produce(jobs, staffing * b.powered.clamp(0.0, 1.0));
                }
            }
        }
        // Walk-in trade: a share of the people who passed a shop this hour
        // step inside and buy something off the shelf.
        for &bid in &ids {
            let (lot, jobs) = (self.buildings[bid].lot, self.buildings[bid].jobs);
            let feet = self.lots.get(lot).map_or(0, |l| l.footfall_hour);
            if feet == 0 {
                continue;
            }
            let b = &mut self.buildings[bid];
            let Some(bz) = b.business.as_mut() else { continue };
            if bz.closed {
                continue;
            }
            let output = match bz.kind {
                BusinessKind::Grocery => Resource::Groceries,
                BusinessKind::Bakery => Resource::BakedGoods,
                BusinessKind::GadgetShop => Resource::Goods,
                _ => continue,
            };
            let want = feet as f32 * 0.06;
            let got = bz.take(output, want.min(bz.stock_of(output)));
            if got > 0.0 {
                let price = bz.sell_price(output, jobs);
                bz.money += got * price;
                bz.revenue_today += got * price;
                bz.deals += 1;
                self.economy.walk_in_sales_today += got * price;
                if output == Resource::BakedGoods {
                    self.economy.market.meals_sold_today += got as u32;
                }
            }
        }
        for l in self.lots.values_mut() {
            l.footfall_hour = 0;
        }
        // Restocking: find the cheapest delivered supplier or import.
        for &bid in &ids {
            let (kind, jobs, needs, at, closed) = {
                let b = &self.buildings[bid];
                let Some(bz) = &b.business else { continue };
                let Some(lot) = self.lots.get(b.lot) else { continue };
                let needs: Vec<(Resource, f32)> = bz
                    .kind
                    .recipe()
                    .inputs
                    .iter()
                    .filter_map(|(r, _)| {
                        let target = bz.target_input(*r, b.jobs);
                        let have = bz.stock_of(*r) + bz.inbound_of(*r);
                        if have < target * 0.5 { Some((*r, target - have)) } else { None }
                    })
                    .collect();
                (bz.kind, b.jobs, needs, lot.center, bz.closed)
            };
            let _ = (kind, jobs);
            if closed {
                continue;
            }
            for (r, amount) in needs {
                let amount = amount.min(400.0);
                let seller = {
                    let sellers = self.buildings.iter().filter(|(id, _)| *id != bid).filter_map(|(id, b)| {
                        let bz = b.business.as_ref()?;
                        let lot = self.lots.get(b.lot)?;
                        Some((id, bz, b.jobs, lot.center))
                    });
                    best_seller(sellers, r, at, amount)
                };
                match seller {
                    Some((seller_id, price)) => {
                        let got = self.buildings[seller_id].business.as_mut().unwrap().take(r, amount);
                        if got <= 0.0 {
                            continue;
                        }
                        let cost = got * price;
                        {
                            let sb = self.buildings[seller_id].business.as_mut().unwrap();
                            sb.money += cost;
                            sb.revenue_today += cost;
                            sb.deals += 1;
                        }
                        {
                            let bb = self.buildings[bid].business.as_mut().unwrap();
                            bb.money -= cost;
                            bb.spend_today += cost;
                        }
                        self.economy.market.deals_today += 1;
                        self.ship(seller_id, bid, r, got, cost);
                    }
                    None => {
                        let cost = amount * r.base_price() * IMPORT_MARKUP;
                        {
                            let bb = self.buildings[bid].business.as_mut().unwrap();
                            bb.money -= cost;
                            bb.spend_today += cost;
                        }
                        self.economy.market.imports_today += cost;
                        self.import(bid, r, amount, cost);
                    }
                }
            }
        }
        // Exports: surplus outputs leave town at a discount, less of one
        // when a freight yard ships them by rail.
        let yards: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.rail.as_ref().is_some_and(|l| l.kind == RailFacility::FreightYard) && !b.is_derelict()).map(|(id, _)| id).collect();
        let export_bonus = if yards.is_empty() { 1.0 } else { rail::RAIL_EXPORT_BONUS };
        let mut shipped = 0.0f32;
        for &bid in &ids {
            let b = &mut self.buildings[bid];
            let jobs = b.jobs;
            let Some(bz) = b.business.as_mut() else { continue };
            if bz.closed {
                continue;
            }
            for (r, per) in bz.kind.recipe().outputs {
                if r.is_retail() {
                    continue;
                }
                let keep = per * jobs as f32 * crate::market::JOB_RATE * 48.0;
                let have = bz.stock_of(*r);
                if have > keep * 1.2 {
                    let surplus = have - keep;
                    bz.take(*r, surplus);
                    let income = surplus * r.base_price() * EXPORT_DISCOUNT * export_bonus;
                    shipped += surplus;
                    bz.money += income;
                    bz.revenue_today += income;
                    self.economy.market.exports_today += income;
                }
            }
        }
        if let Some(&y) = yards.first() {
            self.buildings[y].rail_outbound += shipped;
        }
        // Statistics.
        let mut sums: std::collections::BTreeMap<Resource, (f32, u32, f32)> = std::collections::BTreeMap::new();
        let mut businesses = 0;
        for b in self.buildings.values() {
            let Some(bz) = &b.business else { continue };
            if bz.closed {
                continue;
            }
            businesses += 1;
            for r in Resource::ALL {
                let e = sums.entry(r).or_insert((0.0, 0, 0.0));
                e.2 += bz.stock_of(r);
                if bz.sells(r) {
                    e.0 += bz.sell_price(r, b.jobs);
                    e.1 += 1;
                }
            }
        }
        let m = &mut self.economy.market;
        m.businesses = businesses;
        for (r, (sum, n, stock)) in sums {
            m.avg_price.insert(r, if n > 0 { sum / n as f32 } else { r.base_price() });
            m.stock.insert(r, stock);
        }
        m.hungry = self.citizens.values().filter(|c| c.satiety < 0.3).count() as u32;
        let _ = tick;
    }

    /// Send a truck from one business to another; deliver instantly when no
    /// road route exists.
    fn ship(&mut self, from: BuildingId, to: BuildingId, resource: Resource, amount: f32, price: f32) {
        let cargo = Cargo { from, to, resource, amount, price };
        if let Some(bz) = self.buildings.get_mut(to).and_then(|b| b.business.as_mut()) {
            *bz.inbound.entry(resource).or_insert(0.0) += amount;
        }
        if self.truck_count() >= crate::market::MAX_TRUCKS {
            self.deliver(cargo);
            return;
        }
        let route = (|| {
            let fl = self.building_lot(from)?;
            let tl = self.building_lot(to)?;
            let fp = fl.entrance();
            let tp = tl.entrance();
            self.traffic.net.find_route(fl.front_segment, fp, tl.front_segment, tp)
        })();
        match route {
            Some((route, s0, s1)) => {
                let color = 7;
                self.traffic.spawn_car(CitizenId::default(), Some(cargo), route, s0, s1, color);
            }
            None => self.deliver(cargo),
        }
    }

    /// Import from outside: a truck enters at the city edge.
    fn truck_count(&self) -> usize {
        self.traffic.cars.values().filter(|c| c.cargo.is_some()).count()
    }

    fn import(&mut self, to: BuildingId, resource: Resource, amount: f32, price: f32) {
        let cargo = Cargo { from: to, to, resource, amount, price };
        if let Some(bz) = self.buildings.get_mut(to).and_then(|b| b.business.as_mut()) {
            *bz.inbound.entry(resource).or_insert(0.0) += amount;
        }
        // With a freight yard in town the goods come by rail, cheaper and
        // later: the order waits at the yard for the next train.
        if let Some(yard) = self.nearest_yard(to) {
            let refund = price * (1.0 - rail::RAIL_IMPORT_FACTOR);
            if let Some(bz) = self.buildings.get_mut(to).and_then(|b| b.business.as_mut()) {
                bz.money += refund;
                bz.spend_today -= refund;
            }
            self.economy.market.imports_today -= refund;
            self.buildings[yard].rail_inbound.push(Cargo { from: yard, to, resource, amount, price: price * rail::RAIL_IMPORT_FACTOR });
            return;
        }
        if self.truck_count() >= crate::market::MAX_TRUCKS {
            self.deliver(cargo);
            return;
        }
        let route = (|| {
            let entry = self.entry_node_const()?;
            let node = self.roads.node(entry);
            let seg = *node.segments.iter().find(|&&s| self.roads.segment(s).alive)?;
            let tl = self.building_lot(to)?;
            let tp = tl.entrance();
            self.traffic.net.find_route(seg, node.pos, tl.front_segment, tp)
        })();
        match route {
            Some((route, s0, s1)) => {
                self.traffic.spawn_car(CitizenId::default(), Some(cargo), route, s0, s1, 6);
            }
            None => self.deliver(cargo),
        }
    }

    fn entry_node_const(&self) -> Option<NodeId> {
        let leaves = self.roads.leaf_nodes();
        let max = leaves.iter().map(|&n| self.roads.node(n).pos.length()).fold(0.0, f32::max);
        leaves.into_iter().find(|&n| self.roads.node(n).pos.length() >= max * 0.6)
    }

    fn deliver(&mut self, cargo: Cargo) {
        if let Some(b) = self.buildings.get_mut(cargo.to) {
            if let Some(bz) = b.business.as_mut() {
                bz.add(cargo.resource, cargo.amount);
                let left = (bz.inbound_of(cargo.resource) - cargo.amount).max(0.0);
                bz.inbound.insert(cargo.resource, left);
            }
        }
    }

    fn daily_citizen(&mut self, cid: CitizenId) {
        let (income, home) = {
            let c = &self.citizens[cid];
            (c.income, c.home)
        };
        let rent = home.and_then(|h| self.buildings.get(h)).map(|b| b.rent).unwrap_or(0.0);
        let tick = self.clock.tick;
        let (stress, class, savings) = {
            let c = &mut self.citizens[cid];
            c.savings += income - rent - 20.0;
            self.economy.tax_today += income * 0.12;
            // Wheels: a bike is cheap and first; a car needs a middle income and
            // savings; the poor sell a car they cannot keep.
            if !c.owns_bike && c.savings > 60.0 && self.rng.random_bool(0.08) {
                c.owns_bike = true;
                c.savings -= 45.0;
                c.remember(tick, "Bought a bike. The city just got smaller.");
            }
            if !c.owns_car && c.class() != Tier::Low && c.savings > 400.0 && self.rng.random_bool(0.04) {
                c.owns_car = true;
                c.savings -= 300.0;
                c.remember(tick, "Bought a car.");
            }
            if c.owns_car && c.class() == Tier::Low && c.savings < -100.0 {
                c.owns_car = false;
                c.savings += 150.0;
                c.remember(tick, "Sold the car. Couldn't afford to keep it.");
            }
            let stressed = rent > income * 0.5;
            c.rent_stress_days = if stressed { c.rent_stress_days.saturating_add(1) } else { 0 };
            c.mood = (c.mood + if stressed { -0.05 } else { 0.01 }).clamp(0.0, 1.0);
            (c.rent_stress_days, c.class(), c.savings)
        };
        {
            let c = &mut self.citizens[cid];
            if c.satiety < 0.25 {
                c.hungry_days = c.hungry_days.saturating_add(1);
                c.mood = (c.mood - 0.1).max(0.0);
            } else {
                c.hungry_days = 0;
            }
            if c.hungry_days >= 4 {
                let tick = self.clock.tick;
                c.remember(tick, "Couldn't keep myself fed here. Leaving.".to_string());
                self.emigrate(cid);
                return;
            }
        }
        if stress >= 3 {
            let cheaper = self.pick_building(|b| b.land_use == LandUse::Residential && b.free_homes() > 0 && b.rent <= income * 0.45);
            match cheaper {
                Some(new_home) if Some(new_home) != home => {
                    self.move_home(cid, new_home, "Rent got too steep. Moved to {}.");
                    self.economy.displaced_total += 1;
                }
                _ => {
                    if savings < -60.0 {
                        self.emigrate(cid);
                    }
                }
            }
            return;
        }
        if savings > 300.0 && self.rng.random::<f32>() < 0.2 {
            let current_tier = home.and_then(|h| self.buildings.get(h)).map(|b| b.tier);
            if let Some(ct) = current_tier {
                if ct < class {
                    let better = self.pick_building(|b| b.land_use == LandUse::Residential && b.tier > ct && b.free_homes() > 0 && b.rent <= income * 0.45);
                    if let Some(new_home) = better {
                        self.move_home(cid, new_home, "Moving up to {}.");
                    }
                }
            }
        }
    }

    fn move_home(&mut self, cid: CitizenId, new_home: BuildingId, template: &str) {
        let tick = self.clock.tick;
        if let Some(old) = self.citizens[cid].home {
            if let Some(b) = self.buildings.get_mut(old) {
                b.residents.retain(|&c| c != cid);
            }
        }
        self.buildings[new_home].residents.push(cid);
        let name = self.buildings[new_home].name.clone();
        let c = &mut self.citizens[cid];
        c.home = Some(new_home);
        c.rent_stress_days = 0;
        c.remember(tick, template.replace("{}", &name));
        if matches!(c.activity, Activity::AtHome) {
            c.next_decision_tick = tick + 1;
        }
    }

    fn emigrate(&mut self, cid: CitizenId) {
        let Some(c) = self.citizens.remove(cid) else { return };
        if let Some(h) = c.home {
            if let Some(b) = self.buildings.get_mut(h) {
                b.residents.retain(|&x| x != cid);
            }
        }
        if let Some(w) = c.work {
            if let Some(b) = self.buildings.get_mut(w) {
                b.workers.retain(|&x| x != cid);
            }
        }
        if let Activity::Driving { car, .. } = c.activity {
            self.traffic.remove_car(car);
        }
        self.economy.emigrated_total += 1;
        self.events.push(SimEvent::CitizenLeft(cid));
    }

    fn develop(&mut self) {
        let vacant: Vec<LotId> = self.lots.iter().filter(|(_, l)| l.building.is_none() && l.usable()).map(|(id, _)| id).collect();
        if vacant.is_empty() {
            return;
        }
        let mut built = 0;
        let start = self.rng.random_range(0..vacant.len());
        // Parking follows the drivers who could not find a space: once a few
        // are circling, someone opens a lot nearby.
        if self.economy.parking_street_today >= 4 && self.rng.random_bool(0.6) {
            if let Some(at) = self.economy.parking_shortage_at {
                // The vacant lot nearest the trouble spot, on land that is not
                // precious and not a park or civic site.
                let mut candidates: Vec<(f32, LotId)> = vacant
                    .iter()
                    .copied()
                    .filter(|&l| !matches!(self.lots[l].land_use, LandUse::Park | LandUse::Civic))
                    .filter(|&l| Tier::for_land_value(self.land_value.at(self.lots[l].center)) != Tier::High)
                    .map(|l| (self.lots[l].center.distance(at), l))
                    .collect();
                candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
                for (d, lot_id) in candidates.into_iter().take(6) {
                    if d > 220.0 {
                        break;
                    }
                    if self.build_parking(lot_id).is_some() {
                        self.economy.parking_street_today = self.economy.parking_street_today.saturating_sub(8);
                        built += 1;
                        break;
                    }
                }
            }
        }
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
        if self.lots.get(lot_id).map_or(true, |l| l.landlocked) {
            return;
        }
        let lu = self.lots[lot_id].land_use;
        let value = self.land_value.at(self.lots[lot_id].center);
        let mut tier = Tier::for_land_value(value);
        // People waiting for somewhere affordable pull new housing down a tier.
        if lu == LandUse::Residential && self.economy.waiting_immigrants > 6 && tier > Tier::Low && self.rng.random_bool(0.5) {
            tier = tier.down();
        }
        let base: u8 = match lu {
            LandUse::Residential => self.rng.random_range(2..=7),
            LandUse::Commercial => self.rng.random_range(2..=8),
            LandUse::Industrial => self.rng.random_range(1..=3),
            LandUse::Entertainment => self.rng.random_range(2..=5),
            LandUse::Civic => self.rng.random_range(2..=5),
            LandUse::Park => 0,
        };
        let floors = match (lu, tier) {
            (LandUse::Park, _) => 0,
            (LandUse::Industrial, _) => base,
            (_, Tier::High) => (base as f32 * 1.8) as u8 + 2,
            (_, Tier::Mid) => base + 1,
            (_, Tier::Low) => base.max(1),
        };
        let seed = self.fresh_seed();
        let name = names::building_name(&mut self.rng, lu);
        let mut b = Building::new(lot_id, lu, floors, tier, seed, name, self.clock.tick, value);
        if !self.lots[lot_id].car_access() {
            b.parking_capacity = 0;
        }
        if let Some(kind) = BusinessKind::candidates(lu).choose(&mut self.rng) {
            let jobs = b.jobs;
            b.business = Some(Business::new(*kind, jobs));
        }
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
            // Income first: a job if one is open, gig money otherwise. Then a
            // home they can afford, the best tier available.
            let job = self.pick_building(|b| b.free_jobs() > 0);
            // Personal skill factor spreads incomes within a job.
            let skill: f32 = self.rng.random_range(0.75..1.35);
            let income = job.and_then(|j| self.buildings.get(j)).map(|b| b.wage() * skill).unwrap_or(40.0 * skill);
            let affordable: Vec<(BuildingId, Tier)> = self
                .buildings
                .iter()
                .filter(|(_, b)| b.land_use == LandUse::Residential && b.free_homes() > 0 && b.rent <= income * 0.5)
                .map(|(id, b)| (id, b.tier))
                .collect();
            let Some(best_tier) = affordable.iter().map(|(_, t)| *t).max() else {
                self.economy.priced_out_total += 1;
                self.economy.waiting_immigrants = (self.economy.waiting_immigrants + 1).min(60);
                break;
            };
            let choices: Vec<BuildingId> = affordable.iter().filter(|(_, t)| *t == best_tier).map(|(id, _)| *id).collect();
            let Some(&home) = choices.choose(&mut self.rng) else { break };
            let seed = self.fresh_seed();
            let name = names::citizen_name(&mut self.rng);
            let tick = self.clock.tick;
            let stations: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.rail.as_ref().is_some_and(|l| l.kind == RailFacility::Station)).map(|(id, _)| id).collect();
            let by_train = if !stations.is_empty() && self.rng.random_bool(0.6) { stations.choose(&mut self.rng).copied() } else { None };
            let pos = match by_train.and_then(|s| self.building_entrance(s)) {
                Some(p) => p,
                None => self.roads.node(entry).pos,
            };
            let (car_chance, bike_chance) = match Tier::for_income(income) {
                Tier::Low => (0.12, 0.6),
                Tier::Mid => (0.6, 0.35),
                Tier::High => (0.92, 0.35),
            };
            let owns_car = self.rng.random_bool(car_chance);
            let owns_bike = self.rng.random_bool(bike_chance);
            let mut citizen = Citizen {
                name,
                seed,
                home: Some(home),
                work: None,
                pos,
                heading: 0.0,
                prev_pos: pos,
                prev_heading: 0.0,
                mode: TravelMode::Walk,
                owns_car,
                owns_bike,
                parked_at: None,
                transit: None,
                activity: Activity::Idle,
                memories: Vec::new(),
                thought: "New city. New me.".to_string(),
                spotlight: false,
                next_decision_tick: tick + self.rng.random_range(5..30),
                arrived_tick: tick,
                inside: None,
                mood: 0.6,
                income,
                savings: self.rng.random_range(20.0..120.0),
                rent_stress_days: 0,
                satiety: 0.7,
                groceries: 2.0,
                hungry_days: 0,
                trail: Vec::new(),
            };
            citizen.remember(tick, format!("Rolled into Neon District on day {} at {}.", self.clock.day() + 1, self.clock.format_hhmm()));
            if let Some(s) = by_train {
                citizen.remember(tick, format!("Stepped off the train at {}.", self.buildings[s].name));
            }
            let id = self.citizens.insert(citizen);
            self.buildings[home].residents.push(id);
            let home_name = self.buildings[home].name.clone();
            self.citizens[id].remember(tick, format!("Moved into {home_name}."));
            if let Some(job) = job {
                self.assign_job(id, job);
            }
            self.economy.waiting_immigrants = self.economy.waiting_immigrants.saturating_sub(1);
            self.travel_to(id, Some(home), Purpose::Home);
            self.events.push(SimEvent::CitizenArrived(id));
        }
    }

    fn assign_job(&mut self, id: CitizenId, job: BuildingId) {
        self.citizens[id].work = Some(job);
        let skill = 0.75 + (self.citizens[id].seed % 1000) as f32 / 1000.0 * 0.6;
        self.citizens[id].income = self.buildings[job].wage() * skill;
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
        let groceries_for_sale = self.retail_available(Resource::Groceries);
        let meals_for_sale = self.retail_available(Resource::BakedGoods);
        let ids: Vec<CitizenId> = self.citizens.keys().collect();

        for id in ids {
            // Walking.
            let arrived = {
                let c = &mut self.citizens[id];
                c.prev_pos = c.pos;
                c.prev_heading = c.heading;
                let speed = if c.mode == TravelMode::Bike { self.config.bike_speed } else { self.config.walk_speed };
                if let Activity::Traveling { path, idx, .. } = &mut c.activity {
                    let mut remaining = speed;
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
                    satiety: c.satiety,
                    groceries: c.groceries,
                    groceries_for_sale,
                    meals_for_sale,
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
        // A park is not a building to go inside: walk its paths to a bench.
        if let (Purpose::Visit(LandUse::Park), Some(b)) = (purpose, to) {
            let lot = self.buildings.get(b).map(|bld| bld.lot);
            let pick = self.rng.random_range(0..64usize);
            if let Some(bench) = lot.and_then(|l| self.walk.bench_in(l, pick)) {
                let here = self.citizens[id].pos;
                let path = match self.walk.route(here, bench.0) {
                    Some((pts, nodes)) => {
                        self.count_footfall(&nodes);
                        pts
                    }
                    None => vec![bench.0],
                };
                let c = &mut self.citizens[id];
                c.mode = TravelMode::Walk;
                c.activity = Activity::Traveling { to: Some(b), purpose: Purpose::Sit, path, idx: 0 };
                return;
            }
        }
        let c = &mut self.citizens[id];
        match (purpose, to) {
            (Purpose::ToStop, Some(_)) => {
                c.activity = Activity::Waiting { at: c.pos };
                c.inside = None;
                c.prev_pos = c.pos;
                if let Some(t) = c.transit.as_mut() {
                    t.since = tick;
                }
                return;
            }
            (Purpose::Sit, Some(b)) => {
                let lot = self.buildings.get(b).map(|bld| bld.lot);
                let facing = lot.and_then(|l| self.lots.get(l)).and_then(|l| l.park.as_ref()).and_then(|p| p.benches.iter().min_by(|x, y| x.0.distance(c.pos).total_cmp(&y.0.distance(c.pos)))).map(|b| b.1);
                c.activity = Activity::Sitting { at: c.pos, park: b };
                c.inside = None;
                if let Some(h) = facing {
                    c.heading = h;
                    c.prev_heading = h;
                }
                c.prev_pos = c.pos;
                if let Some(bld) = self.buildings.get_mut(b) {
                    bld.visitors += 1;
                    let name = bld.name.clone();
                    c.remember(tick, format!("Sat on a bench in {name}."));
                    c.mood = (c.mood + 0.08).min(1.0);
                }
            }
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
                    // Venues charge at the door.
                    if let Some(bz) = bld.business.as_mut() {
                        if bz.kind == BusinessKind::Venue {
                            let cover = 15.0;
                            c.savings -= cover;
                            bz.money += cover;
                            bz.revenue_today += cover;
                        }
                    }
                }
            }
            (Purpose::Shop(r), Some(b)) => {
                c.activity = Activity::Visiting(b);
                c.inside = Some(b);
                if let Some(bld) = self.buildings.get_mut(b) {
                    bld.visitors += 1;
                }
                c.next_decision_tick = tick + 10 + self.rng.random_range(0..20);
                self.buy(id, b, r);
                return;
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
            Intent::Shop(r) => {
                let here = self.citizens[id].pos;
                if let Some(b) = self.best_shop(r, here) {
                    if inside != Some(b) {
                        self.travel_to(id, Some(b), Purpose::Shop(r));
                    } else {
                        self.buy(id, b, r);
                    }
                }
            }
        }
    }

    /// Any business currently stocking a retail resource.
    fn retail_available(&self, r: Resource) -> bool {
        self.buildings.values().any(|b| b.business.as_ref().map_or(false, |bz| !bz.closed && bz.sells(r) && bz.stock_of(r) >= 1.0))
    }

    /// Nearest well-stocked shop for a resource, cheapest within reach.
    fn best_shop(&self, r: Resource, at: Vec2) -> Option<BuildingId> {
        let mut best: Option<(f32, BuildingId)> = None;
        for (id, b) in self.buildings.iter() {
            let Some(bz) = &b.business else { continue };
            if bz.closed || !bz.sells(r) || bz.stock_of(r) < 1.0 {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            let score = bz.sell_price(r, b.jobs) + lot.center.distance(at) * 0.02;
            if best.map_or(true, |(s, _)| score < s) {
                best = Some((score, id));
            }
        }
        best.map(|(_, id)| id)
    }

    /// A citizen buys at a shop they have reached.
    fn buy(&mut self, cid: CitizenId, shop: BuildingId, r: Resource) {
        let tick = self.clock.tick;
        let Some(b) = self.buildings.get_mut(shop) else { return };
        let jobs = b.jobs;
        let name = b.name.clone();
        let Some(bz) = b.business.as_mut() else { return };
        let want = match r {
            Resource::Groceries => 3.0,
            _ => 1.0,
        };
        let price = bz.sell_price(r, jobs);
        let got = bz.take(r, want);
        let c = &mut self.citizens[cid];
        if got <= 0.0 {
            c.remember(tick, format!("Shelves were empty at {name}."));
            c.mood = (c.mood - 0.05).max(0.0);
            return;
        }
        let cost = got * price;
        c.savings -= cost;
        bz.money += cost;
        bz.revenue_today += cost;
        self.economy.market.deals_today += 1;
        match r {
            Resource::Groceries => {
                c.groceries += got;
                self.economy.market.meals_sold_today += got as u32;
            }
            Resource::BakedGoods => {
                c.satiety = (c.satiety + 0.35 * got).min(1.0);
                self.economy.market.meals_sold_today += 1;
                if self.rng.random_bool(0.3) {
                    c.remember(tick, format!("Grabbed a bite at {name}."));
                }
            }
            Resource::Goods => {
                c.mood = (c.mood + 0.12).min(1.0);
                c.remember(tick, format!("Treated myself to something new at {name}."));
            }
            _ => {}
        }
    }

    /// A driver has pulled up at their destination: use the building's own
    /// spaces, else pay at the nearest parking lot with room, else leave the
    /// car on the street and note the shortage.
    fn park_car(&mut self, id: CitizenId) {
        let dest = match &self.citizens[id].activity {
            Activity::Driving { to: Some(b), .. } => *b,
            _ => return,
        };
        let tick = self.clock.tick;
        if let Some(b) = self.buildings.get_mut(dest) {
            if b.parking_free() > 0 {
                b.parking_used += 1;
                self.citizens[id].parked_at = Some(dest);
                return;
            }
        }
        let Some(center) = self.building_lot(dest).map(|l| l.center) else { return };
        let best = self
            .buildings
            .iter()
            .filter(|(_, b)| b.is_parking() && b.parking_free() > 0 && !b.business.as_ref().map_or(true, |z| z.closed))
            .filter_map(|(bid, b)| self.lots.get(b.lot).map(|l| (bid, l.center.distance(center))))
            .filter(|(_, d)| *d < 180.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(bid, _)| bid);
        match best {
            Some(bid) => {
                let (fee, name, step_before) = {
                    let b = &mut self.buildings[bid];
                    let before = b.parking_used * 8 / b.parking_capacity.max(1);
                    b.parking_used += 1;
                    (b.parking_price, b.name.clone(), before)
                };
                let step_after = {
                    let b = &self.buildings[bid];
                    b.parking_used * 8 / b.parking_capacity.max(1)
                };
                if step_after != step_before {
                    self.events.push(SimEvent::BuildingChanged(bid));
                }
                if let Some(bz) = self.buildings[bid].business.as_mut() {
                    bz.money += fee;
                    bz.revenue_today += fee;
                    bz.deals += 1;
                }
                self.economy.parking_fees_today += fee;
                let c = &mut self.citizens[id];
                c.savings -= fee;
                c.parked_at = Some(bid);
                if self.rng.random_bool(0.05) {
                    c.remember(tick, format!("Paid {fee:.0} to park at {name}."));
                }
            }
            None => {
                self.economy.parking_street_today += 1;
                self.economy.parking_shortage_at = Some(center);
                let c = &mut self.citizens[id];
                c.mood = (c.mood - 0.02).max(0.0);
                if self.rng.random_bool(0.08) {
                    c.remember(tick, "Circled for ages. Nowhere to park.");
                }
            }
        }
    }

    /// A driver whose destination lines a footpath leaves the car at the
    /// street and walks the rest. Returns false when there is nothing to walk.
    fn walk_last_leg(&mut self, id: CitizenId) -> bool {
        let (to, purpose) = match &self.citizens[id].activity {
            Activity::Driving { to: Some(b), purpose, .. } => (*b, *purpose),
            _ => return false,
        };
        let Some(lot) = self.building_lot(to) else { return false };
        if lot.car_access() {
            return false;
        }
        let entrance = lot.entrance();
        let here = self.citizens[id].pos;
        let mut path = match self.walk.route(here, entrance) {
            Some((pts, nodes)) => {
                self.count_footfall(&nodes);
                pts
            }
            None => vec![here],
        };
        path.push(entrance);
        let c = &mut self.citizens[id];
        c.mode = TravelMode::Walk;
        c.activity = Activity::Traveling { to: Some(to), purpose, path, idx: 0 };
        true
    }

    fn release_parking(&mut self, id: CitizenId) {
        let Some(bid) = self.citizens[id].parked_at.take() else { return };
        let Some(b) = self.buildings.get_mut(bid) else { return };
        let before = b.parking_used * 8 / b.parking_capacity.max(1);
        b.parking_used = b.parking_used.saturating_sub(1);
        let after = b.parking_used * 8 / b.parking_capacity.max(1);
        if b.is_parking() && before != after {
            self.events.push(SimEvent::BuildingChanged(bid));
        }
    }

    // ── trains ───────────────────────────────────────────────────────────────

    fn facilities(&self, kind: RailFacility) -> Vec<BuildingId> {
        self.buildings.iter().filter(|(_, b)| b.rail.as_ref().is_some_and(|l| l.kind == kind && l.works()) && !b.is_derelict()).map(|(id, _)| id).collect()
    }

    fn nearest_yard(&self, to: BuildingId) -> Option<BuildingId> {
        let p = self.building_entrance(to)?;
        self.facilities(RailFacility::FreightYard).into_iter().filter_map(|y| self.building_entrance(y).map(|q| (y, q.distance(p)))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(y, _)| y)
    }

    /// Set down a station or freight yard at `p`: it fuses to track or
    /// street within reach, lays connector stubs for whatever is missing,
    /// and gets a lot and a building of its own. It runs once both stubs
    /// are joined to the networks.
    pub fn place_rail_facility(&mut self, kind: RailFacility, p: Vec2) -> Result<BuildingId, String> {
        let (facility, rail_fused, road_fused) = Facility::plan(kind, p, &self.roads);
        let cost = match kind {
            RailFacility::Station => rail::STATION_COST,
            RailFacility::FreightYard => rail::YARD_COST,
        };
        if self.economy.treasury < cost {
            return Err(format!("the treasury has {:.0}; a {} costs {:.0}", self.economy.treasury, kind.label(), cost));
        }
        let fp = facility.footprint();
        if self.buildings.values().any(|b| self.lots.get(b.lot).is_some_and(|l| point_in_polygon(l.center, &fp) || fp.iter().any(|c| point_in_polygon(*c, &l.polygon)))) {
            return Err("something is built there".into());
        }
        if self.facilities.values().any(|f| f.center.distance(facility.center) < Facility::size(kind).0 * 0.9) {
            return Err("too close to another station or yard".into());
        }
        self.economy.treasury -= cost;
        let fid = self.facilities.insert(facility.clone());
        // Connector stubs become part of the plan, so they survive rebuilds
        // and roads and track can be snapped onto their ends.
        let had_project = self.project.take();
        if !rail_fused {
            let [a, b] = facility.rail_line();
            self.add_gesture(Gesture::Road { points: vec![a, b], lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: RoadKind::Rail });
        }
        if !road_fused {
            let [a, b] = facility.road_line();
            self.add_gesture(Gesture::Road { points: vec![a, b], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street });
        }
        if self.project.is_some() {
            self.implement();
        } else {
            self.rebuild_from_master();
        }
        self.project = had_project;
        let lot_id = self.facilities[fid].lot.ok_or("the facility lost its lot")?;
        let id = self.attach_rail_building(lot_id, kind)?;
        self.facilities[fid].building = Some(id);
        self.refresh_rail_links();
        Ok(id)
    }

    /// The lot a facility stands on: its footprint, fronting its street.
    fn facility_lot(&self, fid: FacilityId) -> Lot {
        let f = &self.facilities[fid];
        let corners = f.corners();
        let front_mid = f.front_mid();
        let probe = front_mid - f.inward() * rail::ROAD_GAP;
        let (front_segment, front_t, car_point) = match self.roads.nearest_street(probe) {
            Some((sid, t, q)) => (sid, t, q),
            None => (SegmentId(0), 0.5, probe),
        };
        Lot {
            zone: ZoneId::default(),
            land_use: if f.kind == RailFacility::Station { LandUse::Civic } else { LandUse::Industrial },
            polygon: corners.to_vec(),
            corners,
            front_edge: [corners[0], corners[1]],
            center: f.center,
            front_dir: -f.inward(),
            front_segment,
            front_t,
            access_node: self.roads.nearest_endpoint(front_segment, front_t),
            // Left empty: the rebuild matches the old lot's building onto it.
            building: None,
            landlocked: false,
            footfall_today: 0,
            footfall_hour: 0,
            car_segment: Some(front_segment),
            car_point,
            park: None,
        }
    }

    /// Recompute whether each station and yard reaches the railway and the
    /// city's roads.
    fn refresh_rail_links(&mut self) {
        // The railway proper is reached when a trunk end is: a facility's
        // own stub has ends too, and they do not count. The roads are
        // reached when some zoned lot's street is.
        let trunk_ends: Vec<NodeId> = self.roads.rail_leaf_nodes().into_iter().filter(|&n| (self.roads.node(n).pos.y - TRUNK_Y).abs() < 1.0).collect();
        let city_node: Option<NodeId> = self.lots.values().find(|l| l.zone != ZoneId::default() && l.usable()).map(|l| l.access_node);
        let ids: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.rail.is_some()).map(|(id, _)| id).collect();
        for id in ids {
            let (rail_ok, road_ok) = {
                let b = &self.buildings[id];
                let link = b.rail.as_ref().unwrap();
                let seg_ok = self.roads.segment(link.segment).alive && self.roads.segment(link.segment).is_rail();
                let node = self.roads.segment(link.segment).a;
                let rail_ok = seg_ok && trunk_ends.iter().any(|&l| self.roads.rail_path(node, l).is_some_and(|p| p.len() > 1));
                let road_ok = match (self.lots.get(b.lot), city_node) {
                    (Some(l), Some(c)) => self.roads.path(l.access_node, c).is_some(),
                    _ => false,
                };
                (rail_ok, road_ok)
            };
            // The track beside the facility may have been split or re-laid.
            let lot_poly = self.lots.get(self.buildings[id].lot).map(|l| (l.polygon.clone(), l.center, l.front_edge));
            if let Some((poly, center, front)) = lot_poly {
                let kind = self.buildings[id].rail.as_ref().unwrap().kind;
                let length = if kind == RailFacility::Station { rail::STATION_PLATFORM } else { rail::YARD_APRON };
                if let Some(mut fresh) = RailLink::beside(&self.roads, &poly, center, front, kind, length) {
                    fresh.rail_connected = rail_ok;
                    fresh.road_connected = road_ok;
                    self.buildings[id].rail = Some(fresh);
                    continue;
                }
            }
            let link = self.buildings[id].rail.as_mut().unwrap();
            link.rail_connected = rail_ok;
            link.road_connected = road_ok;
        }
    }

    /// The building on a facility's lot.
    fn attach_rail_building(&mut self, lot_id: LotId, kind: RailFacility) -> Result<BuildingId, String> {
        let lot = self.lots.get(lot_id).ok_or("no such lot")?;
        let (jobs, length) = match kind {
            RailFacility::Station => (rail::STATION_JOBS, rail::STATION_PLATFORM),
            RailFacility::FreightYard => (rail::YARD_JOBS, rail::YARD_APRON),
        };
        let link = RailLink::beside(&self.roads, &lot.polygon, lot.center, lot.front_edge, kind, length).ok_or("the track stub did not get laid")?;
        let lu = lot.land_use;
        let value = self.land_value.at(lot.center);
        let seed = self.fresh_seed();
        let name = match kind {
            RailFacility::Station => format!("{} Station", names::building_name(&mut self.rng, LandUse::Civic).split(' ').next().unwrap_or("Central")),
            RailFacility::FreightYard => format!("Yard {}", self.rng.random_range(1..30)),
        };
        let mut b = Building::new(lot_id, lu, 1, Tier::Mid, seed, name, self.clock.tick, value);
        b.rail = Some(link);
        b.homes = 0;
        b.jobs = jobs;
        b.business = None;
        b.parking_capacity = if kind == RailFacility::Station { 12 } else { 4 };
        let id = self.buildings.insert(b);
        self.lots[lot_id].building = Some(id);
        self.events.push(SimEvent::BuildingBuilt(id));
        Ok(id)
    }

    /// Stations within reach of both ends of a long walk: (board, alight,
    /// where to walk to).
    fn plan_rail_trip(&self, from: Vec2, to: Vec2) -> Option<(BuildingId, BuildingId, Vec2)> {
        if from.distance(to) < rail::RAIL_TRIP_MIN {
            return None;
        }
        let stations: Vec<(BuildingId, Vec2)> = self.facilities(RailFacility::Station).into_iter().filter_map(|s| self.building_entrance(s).map(|p| (s, p))).collect();
        if stations.len() < 2 {
            return None;
        }
        let near = |p: Vec2| stations.iter().map(|(id, q)| (*id, *q, q.distance(p))).filter(|(_, _, d)| *d <= rail::RAIL_REACH).min_by(|a, b| a.2.total_cmp(&b.2));
        let (a, pa, da) = near(from)?;
        let (b, _, db) = near(to)?;
        if a == b || da + db + 200.0 > from.distance(to) {
            return None;
        }
        Some((a, b, pa))
    }

    /// Walk to a stop or station and wait there for the ride.
    fn walk_to_transit(&mut self, id: CitizenId, stop: Vec2, leg: TransitLeg, target: BuildingId, purpose: Purpose) {
        let exit_pos = self.leave_building(id);
        let start = exit_pos.unwrap_or(self.citizens[id].pos);
        let mut path = match self.walk.route(start, stop) {
            Some((pts, nodes)) => {
                self.count_footfall(&nodes);
                pts
            }
            None => vec![start],
        };
        path.push(stop);
        let tick = self.clock.tick;
        let c = &mut self.citizens[id];
        if let Some(p) = exit_pos {
            c.pos = p;
            c.prev_pos = p;
        }
        c.mode = TravelMode::Walk;
        c.transit = Some(TransitTrip { leg, target, purpose, since: tick });
        c.activity = Activity::Traveling { to: Some(target), purpose: Purpose::ToStop, path, idx: 0 };
    }

    /// The route a train takes from the west end of the line to the east,
    /// calling at each facility in turn, and where along it each one lies.
    fn plan_train_route(&self, facilities: &[BuildingId]) -> Option<(Vec<NodeId>, Vec<Vec2>, Vec<(BuildingId, f32)>)> {
        let leaves = self.roads.rail_leaf_nodes();
        let entry = leaves.iter().copied().min_by(|a, b| self.roads.node(*a).pos.x.total_cmp(&self.roads.node(*b).pos.x))?;
        let exit = leaves.iter().copied().max_by(|a, b| self.roads.node(*a).pos.x.total_cmp(&self.roads.node(*b).pos.x)).unwrap_or(entry);
        let mut order: Vec<(BuildingId, SegmentId)> = facilities.iter().filter_map(|&f| self.buildings[f].rail.as_ref().map(|l| (f, l.segment))).filter(|(_, s)| self.roads.segment(*s).alive && self.roads.segment(*s).is_rail()).collect();
        order.sort_by(|a, b| self.buildings[a.0].rail.as_ref().unwrap().centre().x.total_cmp(&self.buildings[b.0].rail.as_ref().unwrap().centre().x));
        // Points the train drives through: node positions, plus the platform
        // of the last call, where a train on a dead-end spur turns round
        // instead of running on to the end of the track.
        let mut poly: Vec<Vec2> = vec![self.roads.node(entry).pos];
        let mut nodes = vec![entry];
        let mut cur = entry;
        let push_path = |poly: &mut Vec<Vec2>, nodes: &mut Vec<NodeId>, path: Vec<NodeId>, roads: &RoadNetwork| {
            for n in path.into_iter().skip(1) {
                let p = roads.node(n).pos;
                if poly.last().is_none_or(|q: &Vec2| q.distance(p) > 0.1) {
                    poly.push(p);
                }
                nodes.push(n);
            }
        };
        let last = order.len().saturating_sub(1);
        for (i, (f, seg)) in order.iter().enumerate() {
            let s = self.roads.segment(*seg);
            let pa = self.roads.node(s.a).pos;
            let pb = self.roads.node(s.b).pos;
            let here = self.roads.node(cur).pos;
            let (first, second) = if here.distance(pa) <= here.distance(pb) { (s.a, s.b) } else { (s.b, s.a) };
            if let Some(path) = self.roads.rail_path(cur, first) {
                push_path(&mut poly, &mut nodes, path, &self.roads);
                cur = first;
            }
            let centre = self.buildings[*f].rail.as_ref().unwrap().centre();
            let (_, on_track) = crate::roads::project_on_segment(centre, pa, pb);
            let spur_end = self.roads.neighbors(second).count() <= 1;
            if i == last && spur_end {
                // Run to the platform and come back the same way.
                poly.push(on_track);
            } else if let Some(path) = self.roads.rail_path(cur, second) {
                push_path(&mut poly, &mut nodes, path, &self.roads);
                cur = second;
            }
        }
        let home = if exit != cur { exit } else { entry };
        if let Some(path) = self.roads.rail_path(cur, home) {
            let first_back = self.roads.node(cur).pos;
            if poly.last().is_none_or(|q: &Vec2| q.distance(first_back) > 0.1) {
                poly.push(first_back);
            }
            push_path(&mut poly, &mut nodes, path, &self.roads);
        }
        if poly.len() < 2 {
            return None;
        }
        let mut stops: Vec<(BuildingId, f32)> = Vec::new();
        for (f, _) in &order {
            let c = self.buildings[*f].rail.as_ref().unwrap().centre();
            if let Some((s, d)) = rail::project_on_poly(&poly, c) {
                if d < 8.0 {
                    stops.push((*f, s));
                }
            }
        }
        stops.sort_by(|a, b| a.1.total_cmp(&b.1));
        Some((nodes, poly, stops))
    }

    fn spawn_train(&mut self, kind: TrainKind, facilities: &[BuildingId]) -> bool {
        let Some((nodes, poly, stops)) = self.plan_train_route(facilities) else { return false };
        if stops.is_empty() {
            return false;
        }
        let waiting: usize = facilities.iter().map(|f| self.buildings[*f].rail_inbound.len()).sum();
        let (cars, speed) = match kind {
            TrainKind::Passenger => (4, rail::PASSENGER_SPEED),
            TrainKind::Freight => ((5 + waiting / 3).min(10) as u8, rail::FREIGHT_SPEED),
        };
        let color = self.rng.random_range(0..8u8);
        // Where the route leaves the trunk and where it is back on it.
        // The hold point is the last trunk point before the track leaves
        // the trunk (the switch); the branch is clear again once the train
        // is back on a trunk point.
        let mut branch: Option<(f32, f32)> = None;
        let mut acc = 0.0;
        let mut prev_acc = 0.0;
        let mut off = false;
        for (i, p) in poly.iter().enumerate() {
            if i > 0 {
                acc += poly[i - 1].distance(*p);
            }
            let on_trunk = (p.y - TRUNK_Y).abs() < 1.0;
            if !on_trunk && !off {
                off = true;
                branch = Some(match branch {
                    None => (prev_acc - 12.0, acc + 12.0),
                    Some((a, _)) => (a, acc + 12.0),
                });
            } else if !on_trunk {
                if let Some((a, _)) = branch {
                    branch = Some((a, acc + 12.0));
                }
            } else if off {
                off = false;
                if let Some((a, _)) = branch {
                    branch = Some((a, acc + 12.0));
                }
            }
            prev_acc = acc;
        }
        self.rail.trains.insert(Train { kind, cars, nodes, poly, s: 0.0, prev_s: 0.0, speed, stops, next_stop: 0, dwell: 0.0, passengers: Vec::new(), cargo: Vec::new(), color, held: false, branch });
        true
    }

    /// Dispatch on the headway, move every train, call at stops.
    fn step_rail(&mut self) {
        let tick = self.clock.tick;
        let minute = self.clock.minute_of_day() as u32;
        let seconds = self.config.traffic_seconds_per_tick;
        let stations = self.facilities(RailFacility::Station);
        let yards = self.facilities(RailFacility::FreightYard);
        let queued = self.rail.trains.values().any(|t| t.held && t.branch.is_some_and(|(a, _)| t.s <= a + 0.5));
        let room = self.rail.trains.len() < rail::MAX_TRAINS && !queued;
        if room && Rail::in_service(minute) && !stations.is_empty() && tick.saturating_sub(self.rail.last_passenger_tick) >= rail::PASSENGER_HEADWAY {
            if self.spawn_train(TrainKind::Passenger, &stations) {
                self.rail.stats.passenger_trains_today += 1;
            }
            self.rail.last_passenger_tick = tick;
        }
        let freight_waiting = yards.iter().any(|y| !self.buildings[*y].rail_inbound.is_empty() || self.buildings[*y].rail_outbound > 0.0);
        let since_freight = tick.saturating_sub(self.rail.last_freight_tick);
        if room && !yards.is_empty() && since_freight >= rail::FREIGHT_HEADWAY && (freight_waiting || since_freight >= rail::FREIGHT_HEADWAY * 2) {
            if self.spawn_train(TrainKind::Freight, &yards) {
                self.rail.stats.freight_trains_today += 1;
            }
            self.rail.last_freight_tick = tick;
        }
        let ids: Vec<TrainId> = self.rail.trains.keys().collect();
        for id in ids {
            let blocked = self.rail.blocked(id);
            let branch_busy = self.rail.trains.iter().any(|(oid, o)| oid != id && o.on_branch());
            let arrived = {
                let t = &mut self.rail.trains[id];
                t.prev_s = t.s;
                if t.dwell > 0.0 {
                    t.dwell -= seconds;
                    if t.dwell <= 0.0 {
                        t.next_stop += 1;
                    }
                    continue;
                }
                if blocked {
                    t.held = true;
                    continue;
                }
                t.held = false;
                let target = if t.next_stop < t.stops.len() { t.stops[t.next_stop].1 } else { t.total() };
                // Hold at the junction while another train is on the branch.
                let mut limit = target;
                if let Some((bin, _)) = t.branch {
                    if t.s <= bin + 0.5 && branch_busy {
                        limit = limit.min(bin);
                        t.held = true;
                    }
                }
                t.s = (t.s + t.speed * seconds).min(limit);
                if t.s >= target - 0.01 {
                    if t.next_stop < t.stops.len() { Some(true) } else { Some(false) }
                } else {
                    None
                }
            };
            match arrived {
                Some(true) => self.train_arrived(id),
                Some(false) => {
                    let t = self.rail.trains.remove(id).unwrap();
                    let pos = t.head();
                    for p in t.passengers {
                        if let Some(c) = self.citizens.get_mut(p) {
                            c.transit = None;
                        }
                        self.strand_driver(p, pos);
                    }
                }
                None => {}
            }
        }
        // Riders ride along.
        let heads: Vec<(Vec<CitizenId>, Vec2, f32)> = self.rail.trains.values().map(|t| (t.passengers.clone(), t.head(), t.vehicle_pose(0).1)).collect();
        for (ps, pos, heading) in heads {
            for p in ps {
                if let Some(c) = self.citizens.get_mut(p) {
                    c.pos = pos;
                    c.prev_pos = pos;
                    c.heading = heading;
                }
            }
        }
        let s = &mut self.rail.stats;
        s.trains = self.rail.trains.len() as u32;
        s.stations = stations.len() as u32;
        s.yards = yards.len() as u32;
        s.waiting = self.citizens.values().filter(|c| matches!(c.activity, Activity::Waiting { .. }) && c.transit.is_some_and(|t| matches!(t.leg, TransitLeg::Rail { .. }))).count() as u32;
        s.riding = self.citizens.values().filter(|c| matches!(c.activity, Activity::OnTrain { .. })).count() as u32;
        s.freight_waiting = yards.iter().map(|y| self.buildings[*y].rail_inbound.iter().map(|c| c.amount).sum::<f32>()).sum();
        s.next_passenger_in = if stations.is_empty() { 0 } else { rail::PASSENGER_HEADWAY.saturating_sub(tick.saturating_sub(self.rail.last_passenger_tick)) as u32 };
    }

    /// A train stands at a station or yard: people off and on, goods off.
    fn train_arrived(&mut self, id: TrainId) {
        let tick = self.clock.tick;
        let (at, kind) = {
            let t = &mut self.rail.trains[id];
            t.dwell = rail::DWELL_SECONDS;
            (t.stops[t.next_stop].0, t.kind)
        };
        match kind {
            TrainKind::Passenger => {
                let entrance = self.building_entrance(at).unwrap_or(self.rail.trains[id].head());
                let passengers = self.rail.trains[id].passengers.clone();
                let mut keep: Vec<CitizenId> = Vec::new();
                for p in passengers {
                    let off = self.citizens.get(p).and_then(|c| c.transit).is_none_or(|t| !matches!(t.leg, TransitLeg::Rail { alight, .. } if alight != at));
                    if !off {
                        keep.push(p);
                        continue;
                    }
                    let Some(trip) = self.citizens.get_mut(p).and_then(|c| {
                        c.pos = entrance;
                        c.prev_pos = entrance;
                        c.activity = Activity::Idle;
                        c.transit.take()
                    }) else {
                        continue;
                    };
                    self.travel_to_opts(p, Some(trip.target), trip.purpose, true);
                }
                let boarding: Vec<CitizenId> = self
                    .citizens
                    .iter()
                    .filter(|(_, c)| matches!(c.activity, Activity::Waiting { .. }) && c.transit.is_some_and(|t| matches!(t.leg, TransitLeg::Rail { board, .. } if board == at)))
                    .map(|(id, _)| id)
                    .take(rail::TRAIN_CAPACITY.saturating_sub(keep.len()))
                    .collect();
                let name = self.buildings[at].name.clone();
                for p in boarding {
                    let c = &mut self.citizens[p];
                    c.activity = Activity::OnTrain { train: id };
                    c.savings -= rail::RAIL_FARE;
                    c.remember(tick, format!("Took the train from {name}."));
                    keep.push(p);
                    self.economy.treasury += rail::RAIL_FARE;
                    self.rail.stats.fares_today += rail::RAIL_FARE;
                    self.rail.stats.riders_today += 1;
                }
                self.buildings[at].visitors = self.buildings[at].visitors.saturating_add(1);
                self.rail.trains[id].passengers = keep;
            }
            TrainKind::Freight => {
                let queued = std::mem::take(&mut self.buildings[at].rail_inbound);
                self.buildings[at].rail_outbound = 0.0;
                for cargo in queued {
                    self.rail.stats.freight_in_today += cargo.amount;
                    self.truck_from(at, cargo);
                }
            }
        }
    }

    /// A truck from a yard to the buyer; delivered on the spot when there
    /// is no road route or no trucks to spare.
    fn truck_from(&mut self, from: BuildingId, cargo: Cargo) {
        if self.truck_count() >= crate::market::MAX_TRUCKS {
            self.deliver(cargo);
            return;
        }
        let route = (|| {
            let fl = self.building_lot(from)?;
            let (fs, fp) = (fl.car_segment?, fl.car_point);
            let tl = self.building_lot(cargo.to)?;
            let (ts, tp) = (tl.car_segment?, tl.car_point);
            self.traffic.net.find_route(fs, fp, ts, tp)
        })();
        match route {
            Some((route, s0, s1)) => {
                self.traffic.spawn_car(CitizenId::default(), Some(cargo), route, s0, s1, 6);
            }
            None => self.deliver(cargo),
        }
    }

    // ── buses ────────────────────────────────────────────────────────────────

    /// Place a bus depot on a vacant lot with street access.
    pub fn build_depot(&mut self, lot_id: LotId) -> Result<BuildingId, String> {
        let lot = self.lots.get(lot_id).ok_or("no such lot")?;
        if lot.building.is_some() {
            return Err("that lot is taken".into());
        }
        if lot.landlocked || !lot.car_access() {
            return Err("a depot needs a street".into());
        }
        if matches!(lot.land_use, LandUse::Residential | LandUse::Park) {
            return Err("a depot cannot go on homes or parks".into());
        }
        if lot.area() < transit::DEPOT_MIN_AREA {
            return Err(format!("too small: a depot needs {:.0} m²", transit::DEPOT_MIN_AREA));
        }
        if self.economy.treasury < transit::DEPOT_COST {
            return Err(format!("the treasury has {:.0}; a depot costs {:.0}", self.economy.treasury, transit::DEPOT_COST));
        }
        let lu = lot.land_use;
        let value = self.land_value.at(lot.center);
        self.economy.treasury -= transit::DEPOT_COST;
        let seed = self.fresh_seed();
        let name = format!("Depot {}", self.rng.random_range(1..20));
        let mut b = Building::new(lot_id, lu, 1, Tier::Mid, seed, name, self.clock.tick, value);
        b.depot = true;
        b.homes = 0;
        b.jobs = transit::DEPOT_JOBS;
        b.business = None;
        b.parking_capacity = 0;
        let id = self.buildings.insert(b);
        self.lots[lot_id].building = Some(id);
        self.events.push(SimEvent::BuildingBuilt(id));
        Ok(id)
    }

    pub fn add_bus_route(&mut self) -> RouteId {
        self.transit.new_route()
    }

    /// Append a stop to a line, snapped to the nearest street.
    pub fn add_bus_stop(&mut self, route: RouteId, p: Vec2) -> Result<usize, String> {
        let stop = BusStop::snap(&self.roads, p).ok_or("click next to a street")?;
        let r = self.transit.routes.get_mut(route).ok_or("no such line")?;
        if r.stops.iter().any(|s| s.pos.distance(stop.pos) < 3.0) {
            return Err("there is a stop there already".into());
        }
        r.stops.push(stop);
        let idx = r.stops.len() - 1;
        self.transit.refresh_legs(&self.traffic.net);
        Ok(idx)
    }

    pub fn move_bus_stop(&mut self, route: RouteId, idx: usize, p: Vec2) -> bool {
        let Some(stop) = BusStop::snap(&self.roads, p) else { return false };
        let Some(r) = self.transit.routes.get_mut(route) else { return false };
        if idx >= r.stops.len() {
            return false;
        }
        r.stops[idx] = stop;
        self.transit.refresh_legs(&self.traffic.net);
        true
    }

    /// Remove a stop; a line with no stops left is removed too.
    pub fn remove_bus_stop(&mut self, route: RouteId, idx: usize) {
        let Some(r) = self.transit.routes.get_mut(route) else { return };
        if idx < r.stops.len() {
            r.stops.remove(idx);
        }
        let empty = r.stops.is_empty();
        // Buses heading past the removed stop re-aim at what is now there.
        for car in self.traffic.cars.values_mut() {
            if let Some(bus) = car.bus.as_mut() {
                if bus.route == route && bus.next_stop != usize::MAX && bus.next_stop >= idx {
                    bus.next_stop = bus.next_stop.saturating_sub(1);
                }
            }
        }
        if empty {
            self.remove_bus_route(route);
        } else {
            self.transit.refresh_legs(&self.traffic.net);
        }
    }

    pub fn remove_bus_route(&mut self, route: RouteId) {
        self.transit.routes.remove(route);
        let buses: Vec<CarId> = self.traffic.cars.iter().filter(|(_, c)| c.bus.as_ref().is_some_and(|b| b.route == route)).map(|(id, _)| id).collect();
        for id in buses {
            if let Some(car) = self.traffic.remove_car(id) {
                self.strand_car(car);
            }
        }
        let riders: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.transit.is_some_and(|t| matches!(t.leg, TransitLeg::Bus { route: r, .. } if r == route))).map(|(id, _)| id).collect();
        for id in riders {
            self.citizens[id].transit = None;
            if matches!(self.citizens[id].activity, Activity::Waiting { .. }) {
                self.citizens[id].activity = Activity::Idle;
                self.citizens[id].next_decision_tick = self.clock.tick + 1;
            }
        }
        self.transit.refresh_legs(&self.traffic.net);
    }

    pub fn set_route_buses(&mut self, route: RouteId, n: u8) {
        if let Some(r) = self.transit.routes.get_mut(route) {
            r.buses = n.min(transit::DEPOT_CAPACITY as u8);
            self.transit.version += 1;
        }
    }

    /// Dispatch buses from depots during service, call them home after,
    /// count the system, and let riders who waited too long walk.
    fn step_transit(&mut self) {
        let tick = self.clock.tick;
        let minute = self.clock.minute_of_day();
        let in_service = Transit::in_service(minute as u32);
        let depots: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.depot && !b.is_derelict()).map(|(id, _)| id).collect();
        // Buses out per line, and per depot.
        let mut running: HashMap<RouteId, u32> = HashMap::new();
        let mut at_depot: HashMap<BuildingId, u32> = HashMap::new();
        for car in self.traffic.cars.values() {
            if let Some(bus) = &car.bus {
                if !bus.to_depot {
                    *running.entry(bus.route).or_default() += 1;
                }
                *at_depot.entry(bus.depot).or_default() += 1;
            }
        }
        if in_service && tick.saturating_sub(self.transit.last_dispatch_tick) >= transit::DISPATCH_GAP {
            let want: Option<RouteId> = self.transit.routes.iter().filter(|(id, r)| r.runnable() && running.get(id).copied().unwrap_or(0) < r.buses as u32).map(|(id, _)| id).next();
            if let Some(route) = want {
                // The depot with room nearest the first stop.
                let first = self.transit.routes[route].stops[0];
                let depot = depots
                    .iter()
                    .copied()
                    .filter(|d| at_depot.get(d).copied().unwrap_or(0) < transit::DEPOT_CAPACITY)
                    .filter_map(|d| self.building_lot(d).map(|l| (d, l.car_point.distance(first.pos))))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(d, _)| d);
                if let Some(depot) = depot {
                    if let Some((seg, pt)) = self.building_lot(depot).and_then(|l| l.car_segment.map(|s| (s, l.car_point))) {
                        if let Some((lanes, s0, s1)) = self.traffic.net.find_route(seg, pt, first.segment, first.pos) {
                            let color = self.transit.routes[route].color;
                            let id = self.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, color);
                            self.traffic.cars[id].bus = Some(Bus { route, next_stop: 0, passengers: Vec::new(), to_depot: false, depot });
                            self.transit.last_dispatch_tick = tick;
                        }
                    }
                }
            }
        }
        if !in_service {
            for car in self.traffic.cars.values_mut() {
                if let Some(bus) = car.bus.as_mut() {
                    bus.to_depot = true;
                }
            }
        }
        // Riders who gave up.
        let tired: Vec<CitizenId> = self
            .citizens
            .iter()
            .filter(|(_, c)| matches!(c.activity, Activity::Waiting { .. }) && c.transit.is_some_and(|t| t.since > 0 && tick.saturating_sub(t.since) > if matches!(t.leg, TransitLeg::Rail { .. }) { transit::WAIT_GIVE_UP * 2 } else { transit::WAIT_GIVE_UP }))
            .map(|(id, _)| id)
            .collect();
        for id in tired {
            let Some(trip) = self.citizens[id].transit.take() else { continue };
            self.citizens[id].thought = "No bus is coming. Walking it.".to_string();
            self.travel_to_opts(id, Some(trip.target), trip.purpose, false);
        }
        // The count.
        let s = &mut self.economy.transit;
        s.buses = self.traffic.cars.values().filter(|c| c.bus.is_some()).count() as u32;
        s.depots = depots.len() as u32;
        s.depot_capacity = depots.len() as u32 * transit::DEPOT_CAPACITY;
        s.waiting = self.citizens.values().filter(|c| matches!(c.activity, Activity::Waiting { .. })).count() as u32;
        s.riding = self.citizens.values().filter(|c| matches!(c.activity, Activity::Riding { .. })).count() as u32;
        self.transit.stats = s.clone();
    }

    /// A bus reached the end of a leg: a stop, or the depot.
    fn bus_arrived(&mut self, car_id: CarId) {
        let tick = self.clock.tick;
        let (bus, pos) = {
            let car = &self.traffic.cars[car_id];
            (car.bus.clone().unwrap(), car.pos)
        };
        let gone = |w: &mut World, car_id: CarId| {
            if let Some(car) = w.traffic.remove_car(car_id) {
                w.strand_car(car);
            }
        };
        if bus.next_stop == usize::MAX {
            // Home at the depot.
            gone(self, car_id);
            return;
        }
        let stop_idx = bus.next_stop;
        let (here, fare, n) = match self.transit.routes.get(bus.route) {
            Some(r) if stop_idx < r.stops.len() => (r.stops[stop_idx], r.fare, r.stops.len()),
            _ => {
                gone(self, car_id);
                return;
            }
        };
        // Off.
        let mut passengers = bus.passengers.clone();
        let alighting: Vec<CitizenId> = passengers.iter().copied().filter(|p| self.citizens.get(*p).and_then(|c| c.transit).is_none_or(|t| match t.leg { TransitLeg::Bus { route, alight, .. } => alight == stop_idx || route != bus.route, _ => true })).collect();
        passengers.retain(|p| !alighting.contains(p));
        for p in alighting {
            let Some(trip) = self.citizens.get_mut(p).and_then(|c| {
                c.pos = here.pos;
                c.prev_pos = here.pos;
                c.activity = Activity::Idle;
                c.transit.take()
            }) else {
                continue;
            };
            self.travel_to_opts(p, Some(trip.target), trip.purpose, false);
        }
        // On, up to the capacity.
        let boarding: Vec<CitizenId> = self
            .citizens
            .iter()
            .filter(|(_, c)| matches!(c.activity, Activity::Waiting { .. }) && c.transit.is_some_and(|t| matches!(t.leg, TransitLeg::Bus { route, board, .. } if route == bus.route && board == stop_idx)))
            .map(|(id, _)| id)
            .take(transit::BUS_CAPACITY.saturating_sub(passengers.len()))
            .collect();
        for p in boarding {
            let c = &mut self.citizens[p];
            c.activity = Activity::Riding { bus: car_id };
            c.savings -= fare;
            c.remember(tick, format!("Caught the {} bus.", self.transit.routes[bus.route].name));
            passengers.push(p);
            self.economy.treasury += fare;
            self.economy.transit.fares_today += fare;
            self.economy.transit.riders_today += 1;
            self.transit.routes[bus.route].riders_today += 1;
        }
        // On to the next stop, or home.
        let (next_idx, to_seg, to_pos) = if bus.to_depot {
            match self.building_lot(bus.depot).and_then(|l| l.car_segment.map(|s| (s, l.car_point))) {
                Some((s, p)) => (usize::MAX, s, p),
                None => {
                    gone(self, car_id);
                    return;
                }
            }
        } else {
            let k = (stop_idx + 1) % n;
            let s = self.transit.routes[bus.route].stops[k];
            (k, s.segment, s.pos)
        };
        match self.traffic.net.find_route(here.segment, pos, to_seg, to_pos) {
            Some((lanes, s0, s1)) => {
                self.traffic.reroute(car_id, lanes, s0, s1);
                let car = &mut self.traffic.cars[car_id];
                car.hold = transit::DWELL_SECONDS;
                if let Some(b) = car.bus.as_mut() {
                    b.next_stop = next_idx;
                    b.passengers = passengers;
                }
            }
            None => gone(self, car_id),
        }
    }

    /// Place a power plant on a vacant lot, paying from the treasury.
    pub fn build_power(&mut self, lot_id: LotId, kind: PowerPlantKind) -> Result<BuildingId, String> {
        let lot = self.lots.get(lot_id).ok_or("no such lot")?;
        if lot.building.is_some() {
            return Err("that lot is taken".into());
        }
        if lot.landlocked {
            return Err("that land has no road access".into());
        }
        if !kind.allowed_on(lot.land_use) {
            return Err(format!("a {} cannot go on {} land", kind.label(), lot.land_use.label().to_lowercase()));
        }
        if lot.area() < kind.min_area() {
            return Err(format!("too small: a {} needs {:.0} m²", kind.label(), kind.min_area()));
        }
        if self.economy.treasury < kind.build_cost() {
            return Err(format!("the treasury has {:.0}; a {} costs {:.0}", self.economy.treasury, kind.label(), kind.build_cost()));
        }
        let lu = lot.land_use;
        let center = lot.center;
        let value = self.land_value.at(center);
        self.economy.treasury -= kind.build_cost();
        let seed = self.fresh_seed();
        let name = format!("{} {}", match kind {
            PowerPlantKind::Coal => "Blackridge",
            PowerPlantKind::Solar => "Sunfield",
            PowerPlantKind::Wind => "Westerly",
            PowerPlantKind::Nuclear => "Calder",
        }, self.rng.random_range(1..9));
        let mut b = Building::new(lot_id, lu, 1, Tier::Mid, seed, name, self.clock.tick, value);
        b.plant = Some(kind);
        b.homes = 0;
        b.jobs = kind.jobs();
        b.business = None;
        b.parking_capacity = kind.jobs() / 2;
        let id = self.buildings.insert(b);
        self.lots[lot_id].building = Some(id);
        self.events.push(SimEvent::BuildingBuilt(id));
        self.allocate_power();
        Ok(id)
    }

    /// Share the hour's supply out: plants first, then what the regional
    /// grid will sell; the rich are served first, so cheap districts brown
    /// out first. Buildings whose power state changes are redrawn.
    fn allocate_power(&mut self) {
        let day = self.clock.day_fraction();
        let mut supply: f32 = 0.0;
        let mut plants = 0;
        for b in self.buildings.values() {
            if let Some(kind) = b.plant {
                if !b.is_derelict() {
                    supply += kind.capacity_mw() * kind.availability(&self.weather, day);
                    plants += 1;
                }
            }
        }
        let demand: f32 = self.buildings.values().map(demand_mw).sum::<f32>() + 0.0; // sum of nothing is -0.0
        let shortfall = (demand - supply).max(0.0);
        let imported = shortfall.min(GRID_IMPORT_MW);
        self.economy.power.import_cost_today += imported * GRID_IMPORT_PRICE;
        let mut remaining = supply + imported;
        let mut order: Vec<(BuildingId, f32)> = self.buildings.iter().map(|(id, b)| (id, demand_mw(b))).collect();
        order.sort_by(|a, b| {
            let ta = self.buildings[a.0].tier;
            let tb = self.buildings[b.0].tier;
            tb.cmp(&ta).then(b.1.total_cmp(&a.1))
        });
        let mut unpowered = 0;
        let mut changed = Vec::new();
        for (id, need) in order {
            let b = &mut self.buildings[id];
            let powered = if need <= 0.0 {
                1.0
            } else if remaining >= need {
                remaining -= need;
                1.0
            } else {
                let p = remaining / need;
                remaining = 0.0;
                p
            };
            let before = (b.powered * 2.0).round();
            b.powered = powered;
            if powered < 0.6 {
                b.unpowered_hours_today = b.unpowered_hours_today.saturating_add(1);
                unpowered += 1;
            }
            if (powered * 2.0).round() != before {
                changed.push(id);
            }
        }
        for id in changed {
            self.events.push(SimEvent::BuildingChanged(id));
        }
        let p = &mut self.economy.power;
        p.supply_mw = supply;
        p.demand_mw = demand;
        p.imported_mw = imported;
        p.served = if demand > 0.0 { ((supply + imported) / demand).min(1.0) } else { 1.0 };
        p.plants = plants;
        p.unpowered_buildings = unpowered;
        if p.served < 0.98 {
            p.brownout_hours_today += 1;
        }
        // People in the dark notice.
        let tick = self.clock.tick;
        let ids: Vec<CitizenId> = self.citizens.keys().collect();
        for cid in ids {
            let Some(inside) = self.citizens[cid].inside else { continue };
            let powered = self.buildings.get(inside).map_or(1.0, |b| b.powered);
            if powered < 0.6 {
                let name = self.buildings[inside].name.clone();
                let c = &mut self.citizens[cid];
                c.mood = (c.mood - 0.012).max(0.0);
                if self.rng.random_bool(0.04) {
                    c.thought = "The power is out again.".to_string();
                    c.remember(tick, format!("Sat in the dark at {name}. No power."));
                }
            }
        }
    }

    /// Open a parking business on a vacant lot: a surface lot where land is
    /// cheap, a multi-storey car park in the middle. The cheapest business
    /// there is: two to four jobs and a little float.
    pub fn build_parking(&mut self, lot_id: LotId) -> Option<BuildingId> {
        let lot = self.lots.get(lot_id)?;
        if lot.building.is_some() || lot.landlocked {
            return None;
        }
        let lu = lot.land_use;
        let area = lot.area();
        let center = lot.center;
        let value = self.land_value.at(center);
        let tier = Tier::for_land_value(value).min(Tier::Mid);
        let floors: u8 = if tier == Tier::Mid && area > 500.0 { self.rng.random_range(3..=5) } else { 0 };
        let capacity = if floors == 0 { (area / 28.0) as u16 } else { ((area / 32.0) as u16) * floors as u16 };
        if capacity < 4 {
            return None;
        }
        let seed = self.fresh_seed();
        let name = if floors == 0 { format!("Lot {}", self.rng.random_range(1..99)) } else { format!("ParkHaus {}", self.rng.random_range(1..40)) };
        let mut b = Building::new(lot_id, lu, floors.max(1), tier, seed, name, self.clock.tick, value);
        let jobs = if floors == 0 { 2 } else { 4 };
        b.homes = 0;
        b.jobs = jobs;
        b.floors = floors;
        let mut bz = Business::new(BusinessKind::Parking, jobs);
        bz.money = 80.0;
        b.business = Some(bz);
        b.parking_capacity = capacity;
        b.parking_price = 3.0 * tier.wage_multiplier() * (0.6 + value);
        let id = self.buildings.insert(b);
        self.lots[lot_id].building = Some(id);
        self.events.push(SimEvent::BuildingBuilt(id));
        Some(id)
    }

    fn leave_building(&mut self, id: CitizenId) -> Option<Vec2> {
        self.release_parking(id);
        if matches!(self.citizens[id].activity, Activity::Waiting { .. }) {
            self.citizens[id].activity = Activity::Idle;
        }
        if let Activity::Sitting { park, .. } = self.citizens[id].activity {
            if let Some(b) = self.buildings.get_mut(park) {
                b.visitors = b.visitors.saturating_sub(1);
            }
            self.citizens[id].activity = Activity::Idle;
        }
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
            let entrance = lot.entrance();
            return Some((lot.front_segment, entrance, lot.access_node));
        }
        let (sid, t, q) = self.roads.nearest_on_network(c.pos)?;
        Some((sid, q, self.roads.nearest_endpoint(sid, t)))
    }

    fn travel_to(&mut self, id: CitizenId, target: Option<BuildingId>, purpose: Purpose) {
        self.travel_to_opts(id, target, purpose, true);
    }

    fn travel_to_opts(&mut self, id: CitizenId, target: Option<BuildingId>, purpose: Purpose, allow_bus: bool) {
        let Some(target) = target else { return };
        self.citizens[id].transit = None;
        let Some((from_seg, from_pt, from_node)) = self.origin_for(id) else {
            self.citizens[id].activity = Activity::Idle;
            return;
        };
        let Some(lot) = self.building_lot(target) else { return };
        let (to_seg, to_node) = (lot.front_segment, lot.access_node);
        let entrance = lot.entrance();
        // Where a car can get to: the lot itself, or the nearest street for
        // homes that line a footpath.
        let car_to = lot.car_segment.map(|s| (s, lot.car_point));
        let car_from = match self.citizens[id].inside.and_then(|b| self.building_lot(b)) {
            Some(l) => l.car_segment.map(|s| (s, l.car_point)),
            None => self.roads.nearest_street(from_pt).map(|(s, _, q)| (s, q)),
        };

        let dist = from_pt.distance(entrance);
        let (owns_car, owns_bike) = {
            let c = &self.citizens[id];
            (c.owns_car, c.owns_bike)
        };
        // Park outings on a bike are a pleasure, not a commute: bike owners ride.
        let park_ride = owns_bike && matches!(purpose, Purpose::Visit(LandUse::Park));
        // Drive when the citizen has a car, it is far enough and a lane route exists.
        if let (true, false, true, Some((cf_seg, cf_pt)), Some((ct_seg, ct_pt))) = (owns_car, park_ride, dist >= self.config.drive_threshold, car_from, car_to) {
            if let Some((route, s0, s1)) = self.traffic.net.find_route(cf_seg, cf_pt, ct_seg, ct_pt) {
                let exit_pos = self.leave_building(id);
                let color = self.rng.random_range(0..8u8);
                let car = self.traffic.spawn_car(id, None, route, s0, s1, color);
                let c = &mut self.citizens[id];
                // The short walk from a path-side home to the car is skipped.
                c.pos = if cf_pt.distance(from_pt) > 2.0 { cf_pt } else { exit_pos.unwrap_or(c.pos) };
                c.prev_pos = c.pos;
                c.activity = Activity::Driving { car, to: Some(target), purpose };
                return;
            }
        }
        let _ = (from_seg, to_seg);

        // Otherwise walk along the node graph.
        let Some(nodes) = self.roads.path(from_node, to_node) else {
            let c = &mut self.citizens[id];
            c.thought = "No way through. The grid doesn't connect.".to_string();
            c.activity = Activity::Idle;
            return;
        };
        let seated = matches!(self.citizens[id].activity, Activity::Sitting { .. });
        let mode = if owns_bike && !seated && (park_ride || dist >= self.config.bike_threshold) { TravelMode::Bike } else { TravelMode::Walk };
        let walk_from = if self.citizens[id].inside.is_some() { from_pt } else { self.citizens[id].pos };

        // The train, for long trips on foot between two stations.
        if allow_bus && mode == TravelMode::Walk && Rail::in_service(self.clock.minute_of_day() as u32) {
            if let Some((board, alight, stop)) = self.plan_rail_trip(walk_from, entrance) {
                self.walk_to_transit(id, stop, TransitLeg::Rail { board, alight }, target, purpose);
                return;
            }
        }
        // The bus, for long trips on foot when a line serves both ends and
        // a depot has buses out. Walk to the stop first.
        if allow_bus && mode == TravelMode::Walk && self.economy.transit.buses > 0 {
            if let Some((route, board, alight)) = self.transit.plan_trip(walk_from, entrance) {
                let stop = self.transit.routes[route].stops[board].pos;
                let exit_pos = self.leave_building(id);
                let start = exit_pos.unwrap_or(walk_from);
                let mut path = match self.walk.route(start, stop) {
                    Some((pts, nodes)) => {
                        self.count_footfall(&nodes);
                        pts
                    }
                    None => vec![start],
                };
                path.push(stop);
                let tick = self.clock.tick;
                let c = &mut self.citizens[id];
                if let Some(p) = exit_pos {
                    c.pos = p;
                    c.prev_pos = p;
                }
                c.mode = TravelMode::Walk;
                c.transit = Some(TransitTrip { leg: TransitLeg::Bus { route, board, alight }, target, purpose, since: tick });
                c.activity = Activity::Traveling { to: Some(target), purpose: Purpose::ToStop, path, idx: 0 };
                return;
            }
        }
        let exit_pos = self.leave_building(id);
        let mut path = match mode {
            TravelMode::Walk => match self.walk.route(walk_from, entrance) {
                Some((pts, walk_nodes)) => {
                    self.count_footfall(&walk_nodes);
                    pts
                }
                None => self.route_path(&nodes, mode),
            },
            TravelMode::Bike => self.route_path(&nodes, mode),
        };
        path.push(entrance);
        let c = &mut self.citizens[id];
        if let Some(p) = exit_pos {
            c.pos = p;
            c.prev_pos = p;
        }
        c.mode = mode;
        c.activity = Activity::Traveling { to: Some(target), purpose, path, idx: 0 };
    }

    /// Everyone on foot who passes a lot is a potential customer.
    fn count_footfall(&mut self, walk_nodes: &[u32]) {
        self.economy.walk_trips_today += 1;
        for lid in self.walk.lots_passed(walk_nodes) {
            if let Some(l) = self.lots.get_mut(lid) {
                l.footfall_today += 1;
                l.footfall_hour += 1;
            }
        }
    }

    /// Route along the right-hand side of each leg: walkers in the middle
    /// of the pavement, bikes on the road shoulder; both down the middle of
    /// a path (walkers keep right, bikes keep left).
    fn route_path(&self, nodes: &[NodeId], mode: TravelMode) -> Vec<Vec2> {
        if nodes.len() < 2 {
            return nodes.iter().map(|&n| self.roads.node(n).pos).collect();
        }
        let mut out = Vec::with_capacity(nodes.len() * 2);
        for w in nodes.windows(2) {
            let a = self.roads.node(w[0]).pos;
            let b = self.roads.node(w[1]).pos;
            let d = (b - a).normalize_or_zero();
            let right = Vec2::new(-d.y, d.x);
            let seg = self.roads.neighbors(w[0]).find(|(n, _)| *n == w[1]).map(|(_, sid)| self.roads.segment(sid));
            let offset = match (seg.map(|s| s.kind).unwrap_or(RoadKind::Street), mode) {
                (RoadKind::Street, TravelMode::Walk) => seg.map_or(5.0, |s| s.half_width()) + crate::zoning::SIDEWALK * 0.55,
                (RoadKind::Street, TravelMode::Bike) => seg.map_or(5.0, |s| s.half_width()) - crate::roads::SHOULDER * 0.5,
                (RoadKind::Path, TravelMode::Walk) => 0.6,
                (RoadKind::Path, TravelMode::Bike) => -0.5,
                (RoadKind::Rail, _) => 0.0,
            };
            let off = right * offset;
            out.push(a + off);
            out.push(b + off);
        }
        out
    }

    fn wander(&mut self, id: CitizenId) {
        let Some((_, _, start)) = self.origin_for(id) else { return };
        if self.roads.nodes.is_empty() {
            return;
        }
        // Bike owners who are comfortable take joy rides to a park when there is one.
        let (owns_bike, class) = {
            let c = &self.citizens[id];
            (c.owns_bike, c.class())
        };
        if owns_bike && (class == Tier::High || self.rng.random_bool(0.35)) {
            let parks: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.land_use == LandUse::Park).map(|(id, _)| id).collect();
            if let Some(&park) = parks.choose(&mut self.rng) {
                self.travel_to(id, Some(park), Purpose::Visit(LandUse::Park));
                return;
            }
        }
        let mode = if owns_bike && self.rng.random_bool(0.5) { TravelMode::Bike } else { TravelMode::Walk };
        let here = self.citizens[id].pos;
        let walk_route = if mode == TravelMode::Walk && !self.walk.nodes.is_empty() {
            let target = self.walk.nodes[self.rng.random_range(0..self.walk.nodes.len())].pos;
            self.walk.route(here, target)
        } else {
            None
        };
        let path = match walk_route {
            Some((pts, walk_nodes)) => {
                self.count_footfall(&walk_nodes);
                self.leave_building(id);
                pts
            }
            None => {
                let end = NodeId(self.rng.random_range(0..self.roads.nodes.len() as u32));
                let Some(nodes) = self.roads.path(start, end) else { return };
                self.leave_building(id);
                self.route_path(&nodes, mode)
            }
        };
        let exit_pos: Option<Vec2> = None;
        let c = &mut self.citizens[id];
        if let Some(p) = exit_pos {
            c.pos = p;
            c.prev_pos = p;
        }
        c.mode = mode;
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
    use crate::market::Resource;
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
        let lot = w.lots.iter().find(|(_, l)| l.usable()).map(|(id, _)| id).unwrap();
        w.build_on(lot);
        let building = w.lots[lot].building.unwrap();

        let id = w.add_gesture(Gesture::Road { points: vec![Vec2::new(300.0, -300.0), Vec2::new(300.0, 300.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street });
        assert_eq!(w.preview().planned_roads.len(), 1);
        assert!(w.undo());
        assert_eq!(w.preview().planned_roads.len(), 0);
        assert!(w.redo());
        w.set_gesture(id, Gesture::Road { points: vec![Vec2::new(320.0, -300.0), Vec2::new(320.0, 300.0)], lanes_fwd: 2, lanes_bwd: 2, curved: false, kind: RoadKind::Street }, true);
        w.implement();
        assert!(!w.has_project());
        assert_eq!(w.master.gestures.len(), 4, "two roads, a zone and the trunk line");
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
    fn supply_chain_feeds_the_city() {
        let mut w = World::new(WorldConfig::default());
        for i in -1..=1 {
            let c = i as f32 * 200.0;
            w.add_road(&[Vec2::new(-500.0, c), Vec2::new(500.0, c)], 1, 1);
            w.add_road(&[Vec2::new(c, -500.0), Vec2::new(c, 500.0)], 1, 1);
        }
        w.add_zone(square(-190.0, -190.0, 180.0), LandUse::Residential);
        w.add_zone(square(10.0, 10.0, 180.0), LandUse::Commercial);
        w.add_zone(square(-190.0, 10.0, 180.0), LandUse::Industrial);
        w.add_zone(square(10.0, -190.0, 180.0), LandUse::Industrial);
        let mut minds = IdleMind;
        for _ in 0..(5 * 24 * 60) {
            w.tick(&mut minds);
        }
        let m = &w.economy.market;
        assert!(m.businesses > 3, "businesses should exist: {}", m.businesses);
        let groceries_in_town: f32 = w.buildings.values().filter_map(|b| b.business.as_ref()).map(|bz| bz.stock_of(Resource::Groceries)).sum();
        assert!(groceries_in_town > 0.0, "grocers should have stock from local or imported inputs");
        let deals: u32 = w.buildings.values().filter_map(|b| b.business.as_ref()).map(|bz| bz.deals).sum();
        assert!(deals > 0 || m.imports_today > 0.0, "either local deals or imports must have happened");
        // Save round-trip keeps businesses and cargo.
        let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
        assert_eq!(back.economy.market.businesses, m.businesses);
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

    #[test]
    fn bike_owners_cycle_long_trips_and_motion_is_tracked() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
        let mut minds = IdleMind;
        for _ in 0..(6 * 60) {
            w.tick(&mut minds);
        }
        let c = w.citizens.keys().next().expect("someone moved in");
        {
            let cz = &mut w.citizens[c];
            cz.owns_car = false;
            cz.owns_bike = true;
            cz.inside = None;
            cz.activity = Activity::Idle;
            cz.pos = Vec2::new(-150.0, 10.0);
        }
        let far = w.buildings.iter().map(|(id, b)| (id, w.lots[b.lot].center)).max_by(|a, b| a.1.x.total_cmp(&b.1.x)).unwrap().0;
        w.travel_to(c, Some(far), Purpose::Work);
        assert_eq!(w.citizens[c].mode, TravelMode::Bike);
        assert!(matches!(w.citizens[c].activity, Activity::Traveling { .. }));
        let before = w.citizens[c].pos;
        w.tick(&mut minds);
        let cz = &w.citizens[c];
        assert!((cz.prev_pos.distance(before)) < 0.01, "previous pose is kept for interpolation");
        let moved = cz.pos.distance(before);
        assert!((moved - w.config.bike_speed).abs() < 0.5, "cycled {moved} m in a tick");
    }

    #[test]
    fn parking_lots_fill_charge_and_release() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Commercial);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Commercial).map(|(id, _)| id).collect();
        assert!(lots.len() >= 2);
        // A shop next to a parking lot.
        w.build_on(lots[0]);
        let shop = w.lots[lots[0]].building.unwrap();
        let lot = w.build_parking(lots[1]).expect("parking lot opens");
        let cap = w.buildings[lot].parking_capacity;
        assert!(cap >= 4 && w.buildings[lot].is_parking() && w.buildings[lot].jobs == 2);
        assert!(w.buildings[lot].parking_price > 0.0);
        // Low-tier shops have no spaces of their own, so a driver pays at the lot.
        let mut minds = IdleMind;
        for _ in 0..(12 * 60) {
            w.tick(&mut minds);
        }
        let c = w.citizens.keys().next().expect("someone arrived");
        w.buildings[shop].parking_capacity = 0;
        {
            let cz = &mut w.citizens[c];
            cz.savings = 100.0;
            cz.activity = Activity::Driving { car: w.traffic.cars.keys().next().unwrap_or_default(), to: Some(shop), purpose: Purpose::Shop(Resource::Goods) };
        }
        let before_money = w.buildings[lot].business.as_ref().unwrap().money;
        let used0 = w.buildings[lot].parking_used;
        w.park_car(c);
        assert_eq!(w.buildings[lot].parking_used, used0 + 1);
        assert_eq!(w.citizens[c].parked_at, Some(lot));
        assert!(w.citizens[c].savings < 100.0, "the driver paid");
        assert!(w.buildings[lot].business.as_ref().unwrap().money > before_money, "the lot earned");
        // Leaving frees the space.
        w.citizens[c].inside = Some(shop);
        w.leave_building(c);
        assert_eq!(w.buildings[lot].parking_used, used0);
        // With the lot full, drivers circle and the shortage is counted.
        w.buildings[lot].parking_used = cap;
        let street0 = w.economy.parking_street_today;
        w.citizens[c].activity = Activity::Driving { car: w.traffic.cars.keys().next().unwrap_or_default(), to: Some(shop), purpose: Purpose::Shop(Resource::Goods) };
        w.park_car(c);
        assert_eq!(w.economy.parking_street_today, street0 + 1);
        assert!(w.citizens[c].parked_at.is_none());
    }

    #[test]
    fn park_visits_end_on_a_bench_and_people_leave_on_foot() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 120.0), LandUse::Park);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
        let park_lot = w.lots.iter().find(|(_, l)| l.usable() && l.land_use == LandUse::Park).map(|(id, _)| id).expect("a park lot");
        assert!(w.lots[park_lot].park.is_some(), "the park is planned");
        assert!(!w.walk.benches.is_empty(), "benches are in the walk graph");
        let home_lot = w.lots.iter().find(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, _)| id).unwrap();
        w.build_on(home_lot);
        w.build_on(park_lot);
        let park = w.lots[park_lot].building.expect("park building");
        let home = w.lots[home_lot].building.unwrap();
        let mut minds = IdleMind;
        for _ in 0..(8 * 60) {
            w.tick(&mut minds);
        }
        let c = w.citizens.keys().next().expect("someone arrived");
        w.citizens[c].inside = Some(home);
        w.citizens[c].activity = Activity::AtHome;
        w.citizens[c].owns_bike = false;
        w.travel_to(c, Some(park), Purpose::Visit(LandUse::Park));
        let mut sat = false;
        for _ in 0..1200 {
            w.tick(&mut minds);
            if matches!(w.citizens[c].activity, Activity::Sitting { .. }) {
                sat = true;
                break;
            }
        }
        assert!(sat, "the visitor ends up on a bench: {:?}", w.citizens[c].activity.label());
        let at = w.citizens[c].pos;
        assert!(w.lots[park_lot].park.as_ref().unwrap().benches.iter().any(|(b, _)| b.distance(at) < 0.5), "sitting on a planned bench");
        assert!(w.citizens[c].is_visible() && w.citizens[c].inside.is_none());
        assert_eq!(w.buildings[park].visitors, 1);
        // Leaving: on foot from the bench, no jump.
        w.travel_to(c, Some(home), Purpose::Home);
        assert_eq!(w.buildings[park].visitors, 0);
        assert!(w.citizens[c].pos.distance(at) < 0.5, "starts from the bench");
        assert_eq!(w.citizens[c].mode, TravelMode::Walk);
        assert!(matches!(w.citizens[c].activity, Activity::Traveling { .. }));
    }

    #[test]
    fn buses_need_a_depot_run_a_loop_and_carry_riders() {
        use crate::transit;
        let mut w = World::new(WorldConfig::default());
        // A block: two east-west streets joined by two north-south ones.
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(-400.0, 140.0), Vec2::new(400.0, 140.0)], 1, 1);
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(-300.0, 140.0)], 1, 1);
        w.add_road(&[Vec2::new(300.0, 0.0), Vec2::new(300.0, 140.0)], 1, 1);
        w.add_zone(vec![Vec2::new(-390.0, 10.0), Vec2::new(-310.0, 10.0), Vec2::new(-310.0, 130.0), Vec2::new(-390.0, 130.0)], LandUse::Residential);
        w.add_zone(vec![Vec2::new(310.0, 10.0), Vec2::new(390.0, 10.0), Vec2::new(390.0, 130.0), Vec2::new(310.0, 130.0)], LandUse::Commercial);
        w.add_zone(vec![Vec2::new(-100.0, -150.0), Vec2::new(100.0, -150.0), Vec2::new(100.0, -10.0), Vec2::new(-100.0, -10.0)], LandUse::Industrial);
        let route = w.add_bus_route();
        // Clockwise round the block, stops on the kerb the bus drives along.
        for p in [Vec2::new(-200.0, 4.0), Vec2::new(200.0, 4.0), Vec2::new(296.0, 70.0), Vec2::new(200.0, 136.0), Vec2::new(-200.0, 136.0), Vec2::new(-296.0, 70.0)] {
            w.add_bus_stop(route, p).expect("stop snaps to the street");
        }
        assert!(w.add_bus_stop(route, Vec2::new(-200.0, 4.0)).is_err(), "no duplicate stops");
        assert!(w.add_bus_stop(route, Vec2::new(0.0, 500.0)).is_err(), "far from any street");
        assert_eq!(w.transit.routes[route].stops.len(), 6);
        assert!(w.transit.routes[route].runnable(), "every leg has a road: {:?}", w.transit.routes[route].broken);
        assert!(w.transit.routes[route].legs.iter().all(|l| l.len() >= 2));
        let mut minds = IdleMind;
        // No depot: no bus, however long we wait.
        for _ in 0..200 {
            w.tick(&mut minds);
        }
        assert_eq!(w.traffic.cars.values().filter(|c| c.bus.is_some()).count(), 0, "buses need a depot");
        let depot_lot = w.lots.iter().find(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Industrial && l.area() >= transit::DEPOT_MIN_AREA).map(|(id, _)| id).expect("industrial lot");
        let home_lot = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential).min_by(|a, b| a.1.center.distance(Vec2::new(-320.0, 70.0)).total_cmp(&b.1.center.distance(Vec2::new(-320.0, 70.0)))).map(|(id, _)| id).unwrap();
        assert!(w.build_depot(home_lot).is_err(), "no depots on homes");
        let depot = w.build_depot(depot_lot).expect("depot built");
        assert!(w.buildings[depot].depot && w.economy.treasury < 60_000.0);
        let mut bus_seen = false;
        let mut stops_passed = 0usize;
        let mut last_next = 0usize;
        for _ in 0..1200 {
            w.tick(&mut minds);
            if let Some(car) = w.traffic.cars.values().find(|c| c.bus.is_some()) {
                bus_seen = true;
                let b = car.bus.as_ref().unwrap();
                if b.next_stop != last_next {
                    stops_passed += 1;
                    last_next = b.next_stop;
                }
            }
        }
        assert!(bus_seen, "a bus left the depot");
        assert!(stops_passed >= 4, "the bus went round the loop: {stops_passed} stops");
        assert!(w.economy.transit.buses >= 1);
        // A rider: lives in the west, works in the east, no bike, no car.
        w.build_on(home_lot);
        let shop_lot = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Commercial).min_by(|a, b| a.1.center.distance(Vec2::new(320.0, 70.0)).total_cmp(&b.1.center.distance(Vec2::new(320.0, 70.0)))).map(|(id, _)| id).unwrap();
        w.build_on(shop_lot);
        let home = w.lots[home_lot].building.unwrap();
        let shop = w.lots[shop_lot].building.unwrap();
        for _ in 0..(6 * 60) {
            w.tick(&mut minds);
        }
        let c = w.citizens.keys().next().expect("someone arrived");
        // Buses sleep at the depot overnight: start the trip mid-morning
        // with a bus out on the line.
        while !(7 * 60..=12 * 60).contains(&(w.clock.minute_of_day() as u32)) || w.economy.transit.buses == 0 {
            w.tick(&mut minds);
        }
        {
            let cz = &mut w.citizens[c];
            cz.inside = Some(home);
            cz.activity = Activity::AtHome;
            cz.owns_bike = false;
            cz.owns_car = false;
            cz.savings = 50.0;
        }
        w.travel_to(c, Some(shop), Purpose::Shop(Resource::Goods));
        assert!(matches!(w.citizens[c].activity, Activity::Traveling { purpose: Purpose::ToStop, .. }), "walks to the stop first: {}", w.citizens[c].activity.label());
        assert!(w.citizens[c].transit.is_some());
        let (mut waited, mut rode) = (false, false);
        let mut arrived = false;
        for _ in 0..3000 {
            w.tick(&mut minds);
            match w.citizens[c].activity {
                Activity::Waiting { .. } => waited = true,
                Activity::Riding { .. } => rode = true,
                _ => {}
            }
            if w.citizens[c].inside == Some(shop) {
                arrived = true;
                break;
            }
        }
        assert!(waited && rode && arrived, "waited {waited} rode {rode} arrived {arrived}: {}", w.citizens[c].activity.label());
        assert!(w.citizens[c].savings < 50.0, "paid the fare");
        assert!(w.transit.routes[route].riders_today >= 1 && w.economy.transit.fares_today > 0.0);
        // Editing: moving and removing stops keeps the line sane; removing the
        // last stop removes the line and its buses.
        assert!(w.move_bus_stop(route, 1, Vec2::new(250.0, 4.0)));
        assert!(w.transit.routes[route].runnable());
        w.remove_bus_stop(route, 3);
        assert_eq!(w.transit.routes[route].stops.len(), 5);
        w.set_route_buses(route, 2);
        assert_eq!(w.transit.routes[route].buses, 2);
        w.remove_bus_route(route);
        assert!(w.transit.routes.is_empty());
        assert_eq!(w.traffic.cars.values().filter(|c| c.bus.is_some()).count(), 0);
        // Round trip keeps the depot.
        let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
        assert!(back.buildings[depot].depot);
    }

    #[test]
    fn trains_call_at_stations_and_yards_bring_freight() {
        use crate::rail::{Facility, RailFacility, TrainKind};
        let mut w = World::new(WorldConfig::default());
        assert!(w.roads.alive_segments().any(|(_, s)| s.is_rail()), "the trunk line is there from the start");
        w.economy.treasury = 200_000.0;
        // A branch off the trunk, curving north then running east, and a
        // street beside it. The drawn corner is square; the built track is not.
        w.add_rail(&[Vec2::new(-200.0, TRUNK_Y), Vec2::new(-100.0, -400.0), Vec2::new(-40.0, -300.0), Vec2::new(0.0, -200.0), Vec2::new(0.0, -60.0), Vec2::new(900.0, -60.0)]);
        assert!(w.roads.alive_segments().filter(|(_, s)| s.is_rail()).count() >= 4, "the branch split the trunk and joined it");
        {
            let branch: Vec<Vec2> = w.master.roads().filter(|(_, _, _, _, k)| *k == RoadKind::Rail).map(|(_, pts, ..)| pts).find(|p| p.len() > 2).unwrap();
            let built = crate::rail::smooth_track(&branch, &w.roads);
            assert!(crate::rail::sharpest_turn(&built) < 0.6, "no sharp bends on the built track: {:.2} rad", crate::rail::sharpest_turn(&built));
            let first_leg = (built[1] - built[0]).normalize_or_zero();
            assert!(first_leg.y.abs() < 0.35, "the branch leaves the trunk along it, not square off it: {first_leg:?}");
        }
        w.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(1000.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(120.0, 0.0), Vec2::new(120.0, 120.0)], 1, 1);
        w.add_road(&[Vec2::new(600.0, 0.0), Vec2::new(600.0, 120.0)], 1, 1);
        w.add_zone(vec![Vec2::new(150.0, 10.0), Vec2::new(330.0, 10.0), Vec2::new(330.0, 90.0), Vec2::new(150.0, 90.0)], LandUse::Residential);
        w.add_zone(vec![Vec2::new(650.0, 10.0), Vec2::new(830.0, 10.0), Vec2::new(830.0, 90.0), Vec2::new(650.0, 90.0)], LandUse::Commercial);
        w.add_zone(vec![Vec2::new(350.0, 10.0), Vec2::new(570.0, 10.0), Vec2::new(570.0, 130.0), Vec2::new(350.0, 130.0)], LandUse::Industrial);
        // A station set down beside the branch fuses to it; its street stub
        // is 26 m short of the main street, so it is not connected yet.
        let (plan, rail_fused, road_fused) = Facility::plan(RailFacility::Station, Vec2::new(240.0, -45.0), &w.roads);
        assert!(rail_fused && !road_fused, "fuses to the track only: rail {rail_fused} road {road_fused}");
        assert!((plan.center.y - (-40.0)).abs() < 0.5 && plan.along.x.abs() > 0.99, "sits along the track with its rail side on it: {:?} {:?}", plan.center, plan.along);
        let station_a = w.place_rail_facility(RailFacility::Station, Vec2::new(240.0, -45.0)).expect("station A");
        let la = w.buildings[station_a].rail.clone().unwrap();
        assert!(la.rail_connected && !la.road_connected, "track yes, road not yet: {la:?}");
        assert!(w.lots.get(w.buildings[station_a].lot).is_some_and(|l| l.front_dir.y > 0.9), "the lot fronts its street stub to the north");
        assert!(w.roads.alive_segments().any(|(_, s)| s.is_street() && w.roads.node(s.a).pos.y > -20.0 && w.roads.node(s.a).pos.y < -17.0), "a street stub was laid along the road side");
        // Joining the stub to the main street connects it.
        let stub_end = w.facilities.values().next().unwrap().road_line()[1];
        w.add_road(&[stub_end, Vec2::new(stub_end.x, 0.0)], 1, 1);
        let la = w.buildings[station_a].rail.clone().unwrap();
        assert!(la.works(), "connected once the stub meets the street: {la:?}");
        // Station B the same way, and a freight yard; both get their roads.
        let station_b = w.place_rail_facility(RailFacility::Station, Vec2::new(740.0, -45.0)).expect("station B");
        let end_b = w.facilities.values().find(|f| f.building == Some(station_b)).unwrap().road_line()[1];
        w.add_road(&[end_b, Vec2::new(end_b.x, 0.0)], 1, 1);
        assert!(w.place_rail_facility(RailFacility::Station, Vec2::new(760.0, -45.0)).is_err(), "too close to station B");
        let yard = w.place_rail_facility(RailFacility::FreightYard, Vec2::new(460.0, -50.0)).expect("yard");
        let end_y = w.facilities.values().find(|f| f.building == Some(yard)).unwrap().road_line()[1];
        w.add_road(&[end_y, Vec2::new(end_y.x, 0.0)], 1, 1);
        assert!(w.buildings[yard].rail.as_ref().unwrap().works());
        assert!(w.buildings[station_b].rail.as_ref().unwrap().works());
        // A free-standing yard far from everything lays both stubs and runs nothing.
        let lone = w.place_rail_facility(RailFacility::FreightYard, Vec2::new(-600.0, 300.0)).expect("lone yard");
        let ll = w.buildings[lone].rail.as_ref().unwrap();
        assert!(!ll.rail_connected && !ll.road_connected, "a yard in a field waits for track and road");
        assert!(w.facilities.values().filter(|f| f.lot.is_some()).count() == 4);
        // A rider from a home by station A to a shop by station B.
        let lot_near = |w: &World, p: Vec2, lu: LandUse| w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == lu).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|(id, _)| id).unwrap();
        let home_lot = lot_near(&w, Vec2::new(240.0, 40.0), LandUse::Residential);
        w.build_on(home_lot);
        let shop_lot = lot_near(&w, Vec2::new(815.0, 40.0), LandUse::Commercial);
        w.build_on(shop_lot);
        let home = w.lots[home_lot].building.unwrap();
        let shop = w.lots[shop_lot].building.unwrap();
        // A passenger train comes within the headway and calls at both stations.
        let mut minds = IdleMind;
        let mut saw_train = false;
        let mut dwelt_at_a = false;
        for _ in 0..600 {
            w.tick(&mut minds);
            for t in w.rail.trains.values() {
                if t.kind == TrainKind::Passenger {
                    saw_train = true;
                    assert_eq!(t.stops.len(), 2, "both connected stations are on the route, the lone yard is not");
                    if t.dwell > 0.0 && t.stops[t.next_stop.min(1)].0 == station_a {
                        dwelt_at_a = true;
                    }
                }
            }
        }
        assert!(saw_train && dwelt_at_a, "train {saw_train} dwelt {dwelt_at_a}");
        assert!(w.rail.stats.passenger_trains_today >= 1);
        for _ in 0..(4 * 60) {
            w.tick(&mut minds);
        }
        while !(7 * 60..=14 * 60).contains(&(w.clock.minute_of_day() as u32)) {
            w.tick(&mut minds);
        }
        let c = w.citizens.keys().next().expect("someone arrived");
        {
            let cz = &mut w.citizens[c];
            cz.inside = Some(home);
            cz.activity = Activity::AtHome;
            cz.owns_bike = false;
            cz.owns_car = false;
            cz.savings = 50.0;
        }
        w.travel_to(c, Some(shop), Purpose::Shop(Resource::Goods));
        assert!(matches!(w.citizens[c].transit.map(|t| t.leg), Some(crate::transit::TransitLeg::Rail { board, alight }) if board == station_a && alight == station_b), "plans a rail leg: {:?}", w.citizens[c].transit);
        let (mut waited, mut rode, mut arrived) = (false, false, false);
        for _ in 0..1500 {
            w.tick(&mut minds);
            match w.citizens[c].activity {
                Activity::Waiting { .. } => waited = true,
                Activity::OnTrain { .. } => rode = true,
                _ => {}
            }
            if w.citizens[c].inside == Some(shop) {
                arrived = true;
                break;
            }
        }
        assert!(waited && rode && arrived, "waited {waited} rode {rode} arrived {arrived}: {}", w.citizens[c].activity.label());
        assert!(w.citizens[c].savings < 50.0 && w.rail.stats.riders_today >= 1);
        // Freight: an import goes to the connected yard, the buyer saves a
        // fifth, and the next freight train brings it in.
        let buyer = shop;
        assert!(w.buildings[buyer].business.is_some(), "the shop is a business that imports");
        let money0 = w.buildings[buyer].business.as_ref().unwrap().money;
        let queued0 = w.buildings[yard].rail_inbound.len();
        w.import(buyer, Resource::Parts, 10.0, 100.0);
        assert_eq!(w.buildings[yard].rail_inbound.len(), queued0 + 1, "the order waits at the yard");
        assert!(w.buildings[lone].rail_inbound.is_empty(), "the unconnected yard gets nothing");
        assert!((w.buildings[buyer].business.as_ref().unwrap().money - money0 - 20.0).abs() < 0.01, "rail import refunds a fifth");
        let mut freight_seen = false;
        for _ in 0..900 {
            w.tick(&mut minds);
            if w.rail.trains.values().any(|t| t.kind == TrainKind::Freight) {
                freight_seen = true;
            }
            if w.buildings[yard].rail_inbound.is_empty() && freight_seen {
                break;
            }
        }
        assert!(freight_seen && w.buildings[yard].rail_inbound.is_empty(), "freight train {freight_seen}, waiting {}", w.buildings[yard].rail_inbound.len());
        assert!(w.rail.stats.freight_in_today >= 10.0);
        // Round trip keeps the facilities and their lots.
        let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
        assert!(back.buildings[station_a].rail.is_some() && back.buildings[yard].rail.is_some());
        assert_eq!(back.facilities.len(), 4);
    }

    #[test]
    fn power_is_rationed_rich_first_and_plants_fix_it() {
        use crate::power::{demand_mw, PowerPlantKind};
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
        let ind: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial).map(|(id, _)| id).collect();
        let res: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, _)| id).collect();
        assert!(ind.len() >= 3 && res.len() >= 2);
        // Placement rules.
        assert!(w.build_power(res[0], PowerPlantKind::Coal).is_err(), "coal is industrial only");
        let big = ind.iter().copied().max_by(|a, b| w.lots[*a].area().total_cmp(&w.lots[*b].area())).unwrap();
        let treasury0 = w.economy.treasury;
        w.economy.treasury = 100.0;
        assert!(w.build_power(big, PowerPlantKind::Solar).is_err(), "no money");
        w.economy.treasury = treasury0;
        // Fill the rest with heavy industry: more demand than the regional grid sells.
        for &l in ind.iter().filter(|&&l| l != big) {
            w.build_on(l);
        }
        for &l in &res {
            w.build_on(l);
        }
        for b in w.buildings.values_mut() {
            b.floors = 30;
        }
        let demand: f32 = w.buildings.values().map(demand_mw).sum();
        assert!(demand > crate::power::GRID_IMPORT_MW, "demand {demand}");
        w.allocate_power();
        let p = &w.economy.power;
        assert!(p.served < 1.0 && p.unpowered_buildings > 0 && p.imported_mw > 0.0, "{p:?}");
        // The highest tiers are served before the lowest.
        let worst_high = w.buildings.values().filter(|b| b.tier == Tier::High).map(|b| b.powered).fold(1.0f32, f32::min);
        let best_low = w.buildings.values().filter(|b| b.tier == Tier::Low && demand_mw(b) > 0.0).map(|b| b.powered).fold(0.0f32, f32::max);
        if w.buildings.values().any(|b| b.tier == Tier::High) && w.buildings.values().any(|b| b.tier == Tier::Low) {
            assert!(worst_high >= best_low, "high {worst_high} vs low {best_low}");
        }
        // A nuclear plant covers everything.
        w.economy.treasury = 200_000.0;
        let plant = w.build_power(big, PowerPlantKind::Nuclear).expect("nuclear plant built");
        assert!(w.buildings[plant].plant == Some(PowerPlantKind::Nuclear) && w.buildings[plant].jobs == 60);
        assert!(w.economy.treasury < 200_000.0);
        let p = &w.economy.power;
        assert_eq!(p.plants, 1);
        assert!(p.served >= 0.999 && p.unpowered_buildings == 0, "{p:?}");
        assert!(w.buildings.values().all(|b| b.powered >= 0.999));
        // Weather only matters to solar and wind.
        let noon = 0.5;
        assert_eq!(PowerPlantKind::Nuclear.availability(&w.weather, noon), 1.0);
        let mut clear = w.weather.clone();
        clear.cloud = 0.0;
        let mut dull = clear.clone();
        dull.cloud = 1.0;
        assert!(PowerPlantKind::Solar.availability(&clear, noon) > PowerPlantKind::Solar.availability(&dull, noon));
        assert_eq!(PowerPlantKind::Solar.availability(&clear, 0.0), 0.0, "no sun at midnight");
        // Round trip keeps the plant.
        let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
        assert_eq!(back.buildings[plant].plant, Some(PowerPlantKind::Nuclear));
    }
}
