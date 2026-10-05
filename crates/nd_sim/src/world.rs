use crate::buildings::{Building, BuildingId, Construction, Tier};
use crate::citizens::{Activity, Citizen, CitizenId, Purpose};
use crate::clock::SimClock;
use crate::economy::{Economy, Flow, GIG_INCOME, OVERDRAFT};
use crate::events::SimEvent;
use crate::land_value::{LandValue, LandValueBuilder};
use crate::market::{Business, BusinessKind, Resource, EXPORT_DISCOUNT, HAULAGE, IMPORT_MARKUP};
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
use crate::zoning::{outlines_differ, point_in_polygon, polygons_overlap};
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

/// A parcel that loses or gains more than this share of its area under a
/// building is treated as a new parcel: the building goes.
pub const EXPROPRIATE_AREA_TOL: f32 = 0.12;
/// ... or whose outline moves more than this many metres anywhere.
pub const EXPROPRIATE_DIST_TOL: f32 = 3.0;
/// Days a low-tier building may stand on high-value land before it is
/// redeveloped at the land's tier.
pub const REDEVELOP_DAYS: u8 = 3;
/// Buildings within this distance count towards a lot's local density.
pub const DENSITY_RADIUS: f32 = 150.0;

/// Walking and cycling speeds in metres per second; the per-tick values in
/// `WorldConfig` are derived from these through `traffic_seconds_per_tick`.
pub const WALK_MPS: f32 = 1.4;
pub const BIKE_MPS: f32 = 4.5;
/// Save format version written into every snapshot. Bump it when a load
/// needs migration; `World::after_load` upgrades older saves.
pub const SAVE_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldConfig {
    pub seed: u64,
    /// Half-size of the buildable square, in metres.
    pub half_extent: f32,
    /// Metres walked per tick. Derived: `WALK_MPS` seconds of walking per
    /// tick, see `metres_per_tick`; recomputed on load, never set by hand.
    pub walk_speed: f32,
    /// Metres cycled per tick, derived like `walk_speed`.
    pub bike_speed: f32,
    /// Trips at least this long are cycled when the citizen owns a bike.
    pub bike_threshold: f32,
    /// Trips at least this long (straight line) are driven, shorter ones walked.
    pub drive_threshold: f32,
    /// Seconds of real motion simulated per tick, for everything that moves:
    /// cars, buses, trains, walkers and cyclists. A tick is a clock minute,
    /// so 60 would be real time. That makes a 500 m walk six ticks and every
    /// trip a blink; 1.5 (the old value) made the same walk four hours of
    /// clock, so people left for work and arrived at closing time and
    /// hunger was driven by walking. The compromise is 4: a 300 m commute
    /// takes a quarter of an hour of clock, a kilometre across town by bus
    /// well under an hour, and vehicles look about four times real speed at
    /// 1x, which reads as a city builder rather than a stopwatch.
    pub traffic_seconds_per_tick: f32,
    /// Physics substeps per tick; keeps each car step near half a second.
    pub traffic_substeps: usize,
    pub immigration_interval: u64,
    pub development_interval: u64,
    /// Upper bound on buildings started per development interval.
    pub max_builds_per_interval: usize,
}

impl WorldConfig {
    /// Metres something moving at `mps` covers in one tick.
    pub fn metres_per_tick(&self, mps: f32) -> f32 {
        mps * self.traffic_seconds_per_tick
    }

    /// Recompute the speeds that follow from `traffic_seconds_per_tick`.
    pub fn derive_speeds(&mut self) {
        self.walk_speed = self.metres_per_tick(WALK_MPS);
        self.bike_speed = self.metres_per_tick(BIKE_MPS);
    }
}

impl Default for WorldConfig {
    fn default() -> Self {
        let mut c = Self {
            seed: 7,
            half_extent: 900.0,
            walk_speed: 0.0,
            bike_speed: 0.0,
            bike_threshold: 40.0,
            drive_threshold: 60.0,
            traffic_seconds_per_tick: 4.0,
            traffic_substeps: 8,
            immigration_interval: 30,
            development_interval: 20,
            max_builds_per_interval: 2,
        };
        c.derive_speeds();
        c
    }
}

/// A land value recomputation in progress, spread over a few ticks so one
/// tick does not carry the whole grid. Cloning a world mid-update drops the
/// partial work: the next cycle starts afresh and the grid merely stays a
/// little older for two hours.
#[derive(Default)]
struct LandValueWork {
    builder: Option<LandValueBuilder>,
}

impl Clone for LandValueWork {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for LandValueWork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LandValueWork({})", if self.builder.is_some() { "pending" } else { "idle" })
    }
}

/// Where in the two-hour cycle the land value strips run. They sit away
/// from the hourly tick (0) and the development ticks so no single tick
/// does everything.
const LAND_VALUE_PHASE: u64 = 20;
const LAND_VALUE_STRIPS: u64 = 5;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct World {
    /// `SAVE_VERSION` at the time of saving; older saves are migrated on load.
    #[serde(default)]
    pub version: u32,
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
    /// The project applied to the master, for the `plan_version` it was
    /// built at; refreshed whenever the plan changes, so reads are free.
    #[serde(skip)]
    plan_cache: Option<(u64, Plan)>,
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
    land_value_work: LandValueWork,
    #[serde(skip)]
    pub events: Vec<SimEvent>,
    next_seed: u64,
    /// Connected component of each road node over street and path
    /// segments (index = node id), rebuilt with the roads.
    #[serde(skip)]
    road_component: Vec<u32>,
    /// Working stations and yards (connected and not derelict), refreshed
    /// when links, buildings or conditions change.
    #[serde(skip)]
    stations: Vec<BuildingId>,
    #[serde(skip)]
    yards: Vec<BuildingId>,
    /// Businesses selling each resource, with their lot centres, for the
    /// hour `seller_index_hour` was built in.
    #[serde(skip)]
    seller_index: HashMap<Resource, Vec<(BuildingId, Vec2)>>,
    #[serde(skip)]
    seller_index_hour: Option<u64>,
    /// Statistics read every tick but recomputed every ten.
    #[serde(skip)]
    available: [bool; 6],
    #[serde(skip)]
    groceries_for_sale: bool,
    #[serde(skip)]
    meals_for_sale: bool,
}

impl World {
    pub fn new(mut config: WorldConfig) -> Self {
        config.derive_speeds();
        let land_value = LandValue::new(config.half_extent + 200.0, 32.0);
        let mut economy = Economy::default();
        economy.treasury = 60_000.0;
        let half = config.half_extent;
        let mut w = Self {
            version: SAVE_VERSION,
            land_value,
            rng: SimRng::seed_from_u64(config.seed),
            config,
            clock: SimClock::starting_at(0, 7, 0),
            master: Plan::default(),
            project: None,
            next_gesture: 1,
            plan_version: 0,
            plan_cache: None,
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
            land_value_work: LandValueWork::default(),
            events: Vec::new(),
            next_seed: 1,
            road_component: Vec::new(),
            stations: Vec::new(),
            yards: Vec::new(),
            seller_index: HashMap::new(),
            seller_index_hour: None,
            available: [false; 6],
            groceries_for_sale: false,
            meals_for_sale: false,
        };
        w.plan_changed();
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

    /// Rebuild derived state that is not serialised, and make a save from
    /// any earlier version safe to run.
    fn after_load(&mut self) {
        // Speeds follow the time scale, so a save from before a retune
        // walks at today's pace.
        self.config.derive_speeds();
        self.validate();
        self.version = SAVE_VERSION;
        self.rebuild_road_components();
        self.traffic.rebuild(&self.roads);
        // Park layouts are saved with their lots (the benches people sit on
        // must survive a load), so they are only planned again when a save
        // from before parks were serialised has park lots without one.
        if self.lots.values().any(|l| l.land_use == LandUse::Park && l.polygon.len() >= 3 && l.park.is_none()) {
            crate::park::plan_parks(&mut self.lots, &self.land_value);
        }
        self.walk = WalkGraph::build(&self.roads, &self.lots, &self.land_value, self.walk.version + 1);
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
        self.refresh_facility_cache();
        self.update_economy();
        self.plan_changed();
        self.events.push(SimEvent::PlanChanged);
    }

    /// Drop every reference to something that is not there: a hand-edited
    /// or truncated save must never index a missing building or citizen.
    fn validate(&mut self) {
        let tick = self.clock.tick;
        // Lots and buildings must agree about who stands where.
        for l in self.lots.values_mut() {
            if l.building.is_some_and(|b| !self.buildings.contains_key(b)) {
                l.building = None;
            }
        }
        let building_ids: std::collections::HashSet<BuildingId> = self.buildings.keys().collect();
        for b in self.buildings.values_mut() {
            b.residents.retain(|c| self.citizens.contains_key(*c));
            b.workers.retain(|c| self.citizens.contains_key(*c));
            b.rail_inbound.retain(|c| building_ids.contains(&c.to));
        }
        for f in self.facilities.values_mut() {
            if f.building.is_some_and(|b| !self.buildings.contains_key(b)) {
                f.building = None;
            }
            if f.lot.is_some_and(|l| !self.lots.contains_key(l)) {
                f.lot = None;
            }
        }
        let buildings = &self.buildings;
        let routes = &self.transit.routes;
        for c in self.citizens.values_mut() {
            let mut lost = false;
            for slot in [&mut c.home, &mut c.work, &mut c.inside, &mut c.parked_at] {
                if slot.is_some_and(|b| !buildings.contains_key(b)) {
                    *slot = None;
                    lost = true;
                }
            }
            let trip_ok = match c.transit.map(|t| t.leg) {
                None => true,
                Some(TransitLeg::Bus { route, board, alight }) => routes.get(route).is_some_and(|r| board < r.stops.len() && alight < r.stops.len()),
                Some(TransitLeg::Rail { board, alight }) => buildings.contains_key(board) && buildings.contains_key(alight),
            } && c.transit.is_none_or(|t| buildings.contains_key(t.target));
            if !trip_ok {
                c.transit = None;
                lost = true;
            }
            let target_gone = match &c.activity {
                Activity::Visiting(b) | Activity::Sitting { park: b, .. } => !buildings.contains_key(*b),
                Activity::Traveling { to: Some(b), .. } | Activity::Driving { to: Some(b), .. } => !buildings.contains_key(*b),
                Activity::AtHome => c.home.is_none(),
                Activity::Working => c.work.is_none(),
                Activity::Waiting { .. } | Activity::Riding { .. } | Activity::OnTrain { .. } => c.transit.is_none(),
                _ => false,
            };
            if lost || target_gone {
                c.activity = Activity::Idle;
                c.inside = None;
                c.next_decision_tick = tick + 1;
            }
        }
        // Vehicles whose ends no longer exist, and riders who do not.
        let cars: Vec<CarId> = self.traffic.cars.iter().filter(|(_, car)| car.cargo.is_some_and(|c| !buildings.contains_key(c.to)) || car.bus.as_ref().is_some_and(|b| !routes.contains_key(b.route))).map(|(id, _)| id).collect();
        for id in cars {
            self.traffic.remove_car(id);
        }
        for car in self.traffic.cars.values_mut() {
            if let Some(bus) = car.bus.as_mut() {
                bus.passengers.retain(|p| self.citizens.contains_key(*p));
            }
        }
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

    /// Every change to master or project goes through here: the version
    /// moves and the effective plan is rebuilt once, so the tools can read
    /// it as often as they like.
    fn plan_changed(&mut self) {
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
    fn edit_project(&mut self, f: impl FnOnce(&mut Project)) {
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

    fn rebuild_from_master(&mut self) {
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
    fn rebuild_road_components(&mut self) {
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
    fn road_component(&self, n: NodeId) -> Option<u32> {
        self.road_component.get(n.0 as usize).copied()
    }

    /// Take every train off the line; its passengers stand where it was
    /// and decide again.
    fn clear_trains(&mut self) {
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

    fn demolish(&mut self, bid: BuildingId) {
        self.demolish_with(bid, None, None);
    }

    /// Take a building down. `at` is where it stood when its lot is already
    /// gone; `memory` replaces the residents' default note about it.
    fn demolish_with(&mut self, bid: BuildingId, at: Option<Vec2>, memory: Option<&str>) {
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
    fn lay_off(&mut self, cid: CitizenId, memory: Option<String>) {
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
    fn skill_of(seed: u64) -> f32 {
        0.75 + (seed % 1000) as f32 / 1000.0 * 0.6
    }

    /// A car taken off the road: its driver, or a bus's riders, stand there.
    fn strand_car(&mut self, car: crate::traffic::Car) {
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
    fn strand_driver(&mut self, cid: CitizenId, pos: Vec2) {
        if let Some(c) = self.citizens.get_mut(cid) {
            c.pos = pos;
            c.inside = None;
            c.activity = Activity::Idle;
            c.pending_footfall.clear();
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

    /// Is `p` inside a station's or yard's footprint? Nothing but the
    /// facility itself is ever built on that ground.
    pub fn under_facility(&self, p: Vec2) -> bool {
        self.facilities.values().any(|f| point_in_polygon(p, &f.footprint()))
    }

    /// How tall the surroundings of a lot justify building: the floors of
    /// the ordinary buildings within `DENSITY_RADIUS` plus the lot's footfall
    /// today. An empty field allows two floors; every ten floors nearby (or
    /// four hundred passers-by) allow one more, so towers only rise where
    /// the block is already busy.
    pub fn height_cap(&self, lot_id: LotId) -> u8 {
        let Some(lot) = self.lots.get(lot_id) else { return 3 };
        let center = lot.center;
        let near_floors: u32 = self
            .buildings
            .values()
            .filter(|b| b.lot != lot_id && b.land_use != LandUse::Park && !b.is_parking() && b.plant.is_none() && !b.depot && b.rail.is_none())
            .filter_map(|b| self.lots.get(b.lot).map(|l| (b.floors as u32, l.center)))
            .filter(|(_, c)| c.distance(center) <= DENSITY_RADIUS)
            .map(|(f, _)| f)
            .sum();
        let density = near_floors as f32 + lot.footfall_today as f32 / 40.0;
        (2.0 + density / 10.0).min(255.0) as u8
    }

    /// Floors for a new building on `lot_id`: the tier's range, held down
    /// by `height_cap` so the first houses in a field are low.
    fn pick_floors(&mut self, lot_id: LotId, lu: LandUse, tier: Tier) -> u8 {
        if lu == LandUse::Park {
            return 0;
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
            (LandUse::Industrial, _) => base,
            (_, Tier::High) => (base as f32 * 1.8) as u8 + 2,
            (_, Tier::Mid) => base + 1,
            (_, Tier::Low) => base.max(1),
        };
        floors.clamp(1, self.height_cap(lot_id).max(1))
    }

    fn available_land_uses(&self) -> [bool; 6] {
        let mut out = [false; 6];
        for b in self.buildings.values().filter(|b| b.construction.is_none()) {
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
        // Periodic work is staggered so no tick carries more than one heavy
        // job: the hour starts at :00, sites advance at :30, land value runs
        // in strips from :20, development and immigration sit between.
        if tick % 60 == 0 {
            self.hourly_economy();
        }
        if tick % 60 == 30 {
            self.step_construction();
        }
        let lv = tick % 120;
        if (LAND_VALUE_PHASE..LAND_VALUE_PHASE + LAND_VALUE_STRIPS).contains(&lv) {
            self.update_land_value_strip((lv - LAND_VALUE_PHASE) as u8);
        }
        if tick % 10 == 7 {
            self.update_economy();
        }
        if (tick + 5) % self.config.development_interval == 0 {
            self.develop();
        }
        if (tick + 15) % self.config.immigration_interval == 0 {
            self.immigrate();
        }
        if tick % 120 == 90 {
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
        e.sites = self.buildings.values().filter(|b| b.construction.is_some()).count() as u32;
        let (hb, jb) = self.buildings.values().filter(|b| b.construction.is_some()).fold((0u32, 0u32), |(h, j), b| {
            let (homes, jobs) = b.planned_capacity();
            (h + homes as u32, j + jobs as u32)
        });
        e.homes_building = hb;
        e.jobs_building = jb;
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
        // Transit and rail counts, and what minds may ask for: read every
        // tick, so they are tallied here rather than in each step.
        let (mut bus_wait, mut rail_wait, mut riding, mut on_train) = (0u32, 0u32, 0u32, 0u32);
        for c in self.citizens.values() {
            match c.activity {
                Activity::Waiting { .. } => {
                    if c.transit.is_some_and(|t| matches!(t.leg, TransitLeg::Rail { .. })) {
                        rail_wait += 1;
                    } else {
                        bus_wait += 1;
                    }
                }
                Activity::Riding { .. } => riding += 1,
                Activity::OnTrain { .. } => on_train += 1,
                _ => {}
            }
        }
        let depots = self.buildings.values().filter(|b| b.depot && !b.is_derelict()).count() as u32;
        let t = &mut self.economy.transit;
        t.buses = self.traffic.cars.values().filter(|c| c.bus.is_some()).count() as u32;
        t.depots = depots;
        t.depot_capacity = depots * transit::DEPOT_CAPACITY;
        t.waiting = bus_wait + rail_wait;
        t.riding = riding;
        self.transit.stats = t.clone();
        let r = &mut self.rail.stats;
        r.waiting = rail_wait;
        r.riding = on_train;
        r.trains = self.rail.trains.len() as u32;
        r.stations = self.stations.len() as u32;
        r.yards = self.yards.len() as u32;
        r.freight_waiting = self.yards.iter().filter_map(|y| self.buildings.get(*y)).map(|b| b.rail_inbound.iter().map(|c| c.amount).sum::<f32>()).sum();
        self.available = self.available_land_uses();
        self.groceries_for_sale = self.retail_available(Resource::Groceries);
        self.meals_for_sale = self.retail_available(Resource::BakedGoods);
    }

    /// Sum accessibility, amenities, nuisances, traffic and neighbours into the
    /// land value grid, then refresh the rent of every building. Done in
    /// `LAND_VALUE_STRIPS` strips on consecutive ticks: roads, then the
    /// buildings in two halves, then lots and cars, then the blur and the
    /// rents. Buildings a strip boundary falls between are seen by exactly
    /// one strip, in slot order, so the result does not depend on timing.
    fn update_land_value_strip(&mut self, strip: u8) {
        if strip == 0 {
            self.land_value_work.builder = Some(LandValueBuilder::new(&self.land_value));
        }
        let Some(mut b) = self.land_value_work.builder.take() else { return };
        match strip {
            0 => {
                for (sid, seg) in self.roads.alive_segments() {
                    let (a, bp) = self.roads.segment_points(sid);
                    let len = a.distance(bp);
                    let steps = (len / 24.0).ceil().max(1.0) as usize;
                    let w = if seg.lanes() >= 4 { 0.04 } else { 0.025 };
                    for i in 0..=steps {
                        b.splat(a.lerp(bp, i as f32 / steps as f32), 110.0, w);
                    }
                }
            }
            1 | 2 => {
                let n = self.buildings.len();
                let half = n.div_ceil(2);
                let range = if strip == 1 { 0..half } else { half..n };
                for bld in self.buildings.values().skip(range.start).take(range.end - range.start) {
                    Self::splat_building(&mut b, bld, &self.lots);
                }
            }
            3 => {
                for lot in self.lots.values() {
                    if lot.building.is_none() {
                        b.splat(lot.center, 60.0, -0.03);
                    }
                }
                for car in self.traffic.cars.values() {
                    b.splat(car.pos, 45.0, -0.004);
                }
            }
            _ => {
                b.finish(0.25, &mut self.land_value);
                let lv = &self.land_value;
                for bld in self.buildings.values_mut() {
                    if let Some(lot) = self.lots.get(bld.lot) {
                        bld.update_rent(lv.at(lot.center));
                    }
                }
                return;
            }
        }
        self.land_value_work.builder = Some(b);
    }

    /// Run every strip at once, for tests and scripts that need the grid now.
    pub fn update_land_value(&mut self) {
        for strip in 0..LAND_VALUE_STRIPS as u8 {
            self.update_land_value_strip(strip);
        }
    }

    /// One building's pull on the land around it.
    fn splat_building(b: &mut LandValueBuilder, bld: &Building, lots: &SlotMap<LotId, Lot>) {
        {
            let Some(lot) = lots.get(bld.lot) else { return };
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
    }

    /// Once a sim day: buildings wear, upgrade or decay; citizens pay rent and
    /// react to it.
    fn daily_update(&mut self) {
        // Treasury: yesterday's taxes came in as they were earned; plant
        // upkeep and imported power are settled now, then the books close.
        let upkeep: f32 = self.buildings.values().filter_map(|b| b.plant).map(|k| k.upkeep_per_day()).sum();
        self.economy.debit(Flow::PlantUpkeep, upkeep);
        let grid = self.economy.power.import_cost_today;
        self.economy.debit(Flow::GridImport, grid);
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
        self.economy.debit(Flow::TransitUpkeep, transit_upkeep);
        self.economy.transit.upkeep_today = transit_upkeep;
        let stations = self.buildings.values().filter(|b| b.rail.as_ref().is_some_and(|l| l.kind == RailFacility::Station)).count() as f32;
        let yards = self.buildings.values().filter(|b| b.rail.as_ref().is_some_and(|l| l.kind == RailFacility::FreightYard)).count() as f32;
        let rail_upkeep = stations * rail::STATION_UPKEEP_PER_DAY + yards * rail::YARD_UPKEEP_PER_DAY;
        self.economy.debit(Flow::RailUpkeep, rail_upkeep);
        self.economy.ledger.roll();
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
        self.economy.materials_today = 0.0;
        self.economy.walk_trips_today = 0;
        self.economy.walk_in_sales_today = 0.0;
        for l in self.lots.values_mut() {
            l.footfall_today = 0;
        }
        let tick = self.clock.tick;
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        for bid in ids {
            if self.buildings[bid].construction.is_some() {
                continue;
            }
            let (lu, tier, value, derelict_before, full, special) = {
                let b = &self.buildings[bid];
                let full = if b.land_use == LandUse::Residential { b.homes > 0 && b.free_homes() == 0 } else { b.jobs > 0 && b.free_jobs() == 0 };
                let value = self.lots.get(b.lot).map(|l| self.land_value.at(l.center)).unwrap_or(b.land_value);
                let special = b.is_parking() || b.plant.is_some() || b.depot || b.rail.is_some();
                (b.land_use, b.tier, value, b.is_derelict(), full, special)
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
            // A cheap building on dear land does not last: after three days
            // in the high band it is redeveloped at the land's tier, so the
            // rent rises and those who cannot pay it move on.
            let redevelop = if lu != LandUse::Park && !special {
                let b = &mut self.buildings[bid];
                b.high_land_days = if tier == Tier::Low && target == Tier::High { b.high_land_days.saturating_add(1) } else { 0 };
                b.high_land_days >= REDEVELOP_DAYS
            } else {
                false
            };
            if redevelop {
                let lot = self.buildings[bid].lot;
                let floors = self.pick_floors(lot, lu, target);
                let b = &mut self.buildings[bid];
                b.high_land_days = 0;
                b.tier = target;
                b.floors = floors.max(1);
                b.condition = 1.0;
                b.built_tick = tick;
                let name = b.name.clone();
                let residents = b.residents.clone();
                for cid in residents {
                    if let Some(c) = self.citizens.get_mut(cid) {
                        c.remember(tick, format!("{name} was torn down and rebuilt for richer tenants. The rent went up with it."));
                    }
                }
                changed = true;
            } else if lu != LandUse::Park && !special {
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
        // Conditions moved: a station or yard may have gone derelict.
        self.refresh_facility_cache();
    }

    /// Businesses pay wages; those that cannot for several days close, and
    /// closed buildings are demolished after a week so the lot can be reused.
    fn daily_businesses(&mut self) {
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        let mut demolish = Vec::new();
        for bid in ids {
            if self.buildings[bid].construction.is_some() {
                continue;
            }
            let revenue = self.buildings[bid].business.as_ref().map_or(0.0, |z| z.revenue_today);
            let (wage_bill, workers, publicly_funded, kind) = {
                let b = &self.buildings[bid];
                let Some(bz) = &b.business else { continue };
                let publicly_funded = matches!(bz.kind, BusinessKind::Office) || b.land_use == LandUse::Civic;
                (b.workers.len() as f32 * b.wage() * 0.6, b.workers.clone(), publicly_funded, bz.kind)
            };
            let b = &mut self.buildings[bid];
            let bz = b.business.as_mut().unwrap();
            if bz.closed {
                // A shut business takes nothing and owes no tax; whatever
                // the inspector still showed from its last day is cleared.
                bz.revenue_today = 0.0;
                bz.spend_today = 0.0;
                bz.broke_days = bz.broke_days.saturating_add(1);
                if bz.broke_days > 7 {
                    demolish.push(bid);
                }
                continue;
            }
            self.economy.tax_today += revenue * 0.05;
            self.economy.credit(Flow::BusinessTax, revenue * 0.05);
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
                    self.lay_off(w, Some(format!("{name} shut down. Out of a job.")));
                    if let Some(c) = self.citizens.get_mut(w) {
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
            if b.construction.is_some() {
                continue;
            }
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
            if b.construction.is_some() {
                continue;
            }
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
        self.refresh_seller_index();
        for &bid in &ids {
            let (kind, jobs, needs, at, closed) = {
                let b = &self.buildings[bid];
                if b.construction.is_some() {
                    continue;
                }
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
                let seller = self.best_seller(r, at, amount, bid);
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
            if b.construction.is_some() {
                continue;
            }
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
        self.drain_stranded_rail_imports();
        // Statistics.
        let mut sums: std::collections::BTreeMap<Resource, (f32, u32, f32)> = std::collections::BTreeMap::new();
        let mut businesses = 0;
        for b in self.buildings.values() {
            let Some(bz) = &b.business else { continue };
            if bz.closed || b.construction.is_some() {
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

    /// Goods waiting at a yard no freight train will reach go by truck:
    /// the yard lost its track or its street, or no freight has run for two
    /// headways (the dispatcher is stuck, say at `MAX_TRAINS`).
    fn drain_stranded_rail_imports(&mut self) {
        let tick = self.clock.tick;
        let stuck = tick.saturating_sub(self.rail.last_freight_tick) >= rail::FREIGHT_HEADWAY * 2;
        let yards: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| !b.rail_inbound.is_empty() && b.rail.as_ref().is_some_and(|l| l.kind == RailFacility::FreightYard)).map(|(id, _)| id).collect();
        for y in yards {
            let works = self.buildings[y].rail.as_ref().is_some_and(|l| l.works()) && !self.buildings[y].is_derelict();
            if works && !stuck {
                continue;
            }
            let queued = std::mem::take(&mut self.buildings[y].rail_inbound);
            for cargo in queued {
                self.dispatch_truck(TruckOrigin::Building(y), cargo, 6);
            }
        }
    }

    /// Who sells what this hour, with their lot centres. Built once an hour
    /// so each buyer's search is a walk over the sellers of one resource,
    /// not over every building; prices and stock are still read live.
    fn refresh_seller_index(&mut self) {
        let hour = self.clock.tick / 60;
        if self.seller_index_hour == Some(hour) {
            return;
        }
        self.seller_index_hour = Some(hour);
        self.seller_index.clear();
        for (id, b) in self.buildings.iter() {
            if b.construction.is_some() {
                continue;
            }
            let Some(bz) = &b.business else { continue };
            if bz.closed {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            for (r, _) in bz.kind.recipe().outputs {
                self.seller_index.entry(*r).or_default().push((id, lot.center));
            }
        }
    }

    /// Best local seller of `r` for a buyer at `at` (other than `buyer`):
    /// cheapest delivered price among those holding at least half the lot.
    fn best_seller(&mut self, r: Resource, at: Vec2, amount: f32, buyer: BuildingId) -> Option<(BuildingId, f32)> {
        self.refresh_seller_index();
        let mut best: Option<(BuildingId, f32)> = None;
        for &(id, pos) in self.seller_index.get(&r).map(|v| v.as_slice()).unwrap_or(&[]) {
            if id == buyer {
                continue;
            }
            let Some(b) = self.buildings.get(id) else { continue };
            let Some(bz) = &b.business else { continue };
            if bz.closed || b.construction.is_some() || bz.stock_of(r) < amount * 0.5 {
                continue;
            }
            let price = bz.sell_price(r, b.jobs) + HAULAGE * pos.distance(at) * r.base_price();
            if best.map_or(true, |(_, p)| price < p) {
                best = Some((id, price));
            }
        }
        best
    }

    /// Send a truck from one business to another; deliver instantly when no
    /// road route exists.
    fn ship(&mut self, from: BuildingId, to: BuildingId, resource: Resource, amount: f32, price: f32) {
        let cargo = Cargo { from, to, resource, amount, price };
        self.note_inbound(to, resource, amount);
        self.dispatch_truck(TruckOrigin::Building(from), cargo, 7);
    }

    fn truck_count(&self) -> usize {
        self.traffic.cars.values().filter(|c| c.cargo.is_some()).count()
    }

    /// The buyer counts a load as on its way so it does not reorder.
    fn note_inbound(&mut self, to: BuildingId, resource: Resource, amount: f32) {
        if let Some(bz) = self.buildings.get_mut(to).and_then(|b| b.business.as_mut()) {
            *bz.inbound.entry(resource).or_insert(0.0) += amount;
        }
    }

    /// Import from outside: by rail through the nearest working yard when
    /// there is one, else a truck from the city edge.
    fn import(&mut self, to: BuildingId, resource: Resource, amount: f32, price: f32) {
        let cargo = Cargo { from: to, to, resource, amount, price };
        self.note_inbound(to, resource, amount);
        // With a freight yard in town the goods come by rail, cheaper and
        // later: the order waits at the yard for the next train. Building
        // sites are the exception: a quarter of the work waiting hours for
        // each load would quadruple build times, so their parts come by
        // the developer's own trucks.
        let site = self.buildings.get(to).is_some_and(|b| b.construction.is_some());
        if !site {
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
        }
        self.dispatch_truck(TruckOrigin::CityEdge, cargo, 6);
    }

    /// Where a truck can pull up at a building: its street, or for a lot
    /// that lines a footpath, the nearest street it was given.
    fn truck_stop(&self, b: BuildingId) -> Option<(SegmentId, Vec2)> {
        let lot = self.building_lot(b)?;
        Some(match lot.car_segment {
            Some(s) => (s, lot.car_point),
            None => (lot.front_segment, lot.entrance()),
        })
    }

    /// Put a truck on the road with `cargo`, from a building or the city
    /// edge. Delivered on the spot when there is no route or the road is
    /// full of trucks already: the buyer counts the load as inbound.
    fn dispatch_truck(&mut self, origin: TruckOrigin, cargo: Cargo, color: u8) {
        if self.truck_count() >= crate::market::MAX_TRUCKS {
            self.deliver(cargo);
            return;
        }
        let route = (|| {
            let (from_seg, from_pt) = match origin {
                TruckOrigin::Building(b) => self.truck_stop(b)?,
                TruckOrigin::CityEdge => {
                    let entry = self.entry_node_const()?;
                    let node = self.roads.node(entry);
                    let seg = *node.segments.iter().find(|&&s| self.roads.segment(s).alive)?;
                    (seg, node.pos)
                }
            };
            let (to_seg, to_pt) = self.truck_stop(cargo.to)?;
            self.traffic.net.find_route(from_seg, from_pt, to_seg, to_pt)
        })();
        match route {
            Some((route, s0, s1)) => {
                self.traffic.spawn_car(CitizenId::default(), Some(cargo), route, s0, s1, color);
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
        let Some(b) = self.buildings.get_mut(cargo.to) else { return };
        if let Some(bz) = b.business.as_mut() {
            let left = (bz.inbound_of(cargo.resource) - cargo.amount).max(0.0);
            bz.inbound.insert(cargo.resource, left);
        }
        // Materials for a building site go onto the site, not the shelves.
        if let Some(c) = b.construction.as_mut() {
            c.deliveries = c.deliveries.saturating_add(1);
            c.ordered = false;
            return;
        }
        if let Some(bz) = b.business.as_mut() {
            bz.add(cargo.resource, cargo.amount);
        }
    }

    // ── construction ─────────────────────────────────────────────────────────

    /// Every sim hour: building sites advance, wait for materials at each
    /// quarter, and open when the work is done.
    fn step_construction(&mut self) {
        let ids: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
        for bid in ids {
            let (order, waiting, before, after) = {
                let c = self.buildings[bid].construction.as_mut().unwrap();
                let before = c.progress;
                // One load is kept on the road ahead of the work, so a
                // quarter only stalls when its truck is late.
                let order = !c.ordered && c.deliveries < Construction::DELIVERIES && c.deliveries <= c.stage() + 1;
                if c.waiting_for_materials() {
                    (order, true, before, before)
                } else {
                    let ceiling = (c.stage() as f32 + 1.0) * 0.25;
                    c.progress = (c.progress + c.hourly_step()).min(ceiling);
                    (order, false, before, c.progress)
                }
            };
            if order {
                self.order_materials(bid);
            }
            if waiting {
                continue;
            }
            if after >= 1.0 {
                self.finish_construction(bid);
            } else if (after * 10.0).floor() > (before * 10.0).floor() {
                self.events.push(SimEvent::BuildingChanged(bid));
            }
        }
    }

    /// A truckload of parts for a building site: from a local fab when one
    /// has stock, else imported. The developer pays, so the money comes from
    /// outside the city like export income does.
    fn order_materials(&mut self, bid: BuildingId) {
        let Some(b) = self.buildings.get(bid) else { return };
        let Some(lot) = self.lots.get(b.lot) else { return };
        let at = lot.center;
        let amount = 4.0 + b.floors as f32 * 2.0;
        let resource = Resource::Parts;
        if let Some(c) = self.buildings[bid].construction.as_mut() {
            c.ordered = true;
        }
        let seller = self.best_seller(resource, at, amount, bid);
        if let Some((seller_id, price)) = seller {
            let got = self.buildings[seller_id].business.as_mut().unwrap().take(resource, amount);
            if got > 0.0 {
                let cost = got * price;
                let sb = self.buildings[seller_id].business.as_mut().unwrap();
                sb.money += cost;
                sb.revenue_today += cost;
                sb.deals += 1;
                self.economy.market.deals_today += 1;
                self.ship(seller_id, bid, resource, got, cost);
                return;
            }
        }
        let cost = amount * resource.base_price() * IMPORT_MARKUP;
        self.economy.materials_today += cost;
        self.import(bid, resource, amount, 0.0);
    }

    /// Open a finished building: the real homes and jobs appear, the site
    /// crew is let go (and steps outside if it was on site). Public so tests
    /// and scripts can skip the wait.
    pub fn finish_construction(&mut self, bid: BuildingId) {
        let tick = self.clock.tick;
        let (car_access, entrance) = match self.buildings.get(bid).and_then(|b| self.lots.get(b.lot)) {
            Some(l) => (l.car_access(), l.entrance()),
            None => (true, Vec2::ZERO),
        };
        let Some(b) = self.buildings.get_mut(bid) else { return };
        if b.construction.take().is_none() {
            return;
        }
        let crew = std::mem::take(&mut b.workers);
        b.recompute_capacity();
        if !car_access {
            b.parking_capacity = 0;
        }
        b.built_tick = tick;
        let name = b.name.clone();
        for cid in crew {
            if self.citizens.get(cid).is_some_and(|c| c.work == Some(bid)) {
                self.lay_off(cid, Some(format!("Topped out {name}. Crew's laid off; looking for the next site.")));
            }
            let Some(c) = self.citizens.get_mut(cid) else { continue };
            if c.inside == Some(bid) {
                c.inside = None;
                c.pos = entrance;
                c.prev_pos = entrance;
                c.activity = Activity::Idle;
                c.next_decision_tick = tick + 1;
            }
        }
        self.events.push(SimEvent::BuildingChanged(bid));
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
            self.economy.credit(Flow::IncomeTax, income * 0.12);
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
        if !self.citizens.contains_key(cid) {
            return;
        }
        self.release_parking(cid);
        let visiting = match self.citizens[cid].activity {
            Activity::Visiting(b) | Activity::Sitting { park: b, .. } => Some(b),
            _ => None,
        };
        if let Some(b) = visiting.and_then(|b| self.buildings.get_mut(b)) {
            b.visitors = b.visitors.saturating_sub(1);
        }
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
        let Some(lot) = self.lots.get(lot_id) else { return };
        if lot.landlocked || lot.building.is_some() || self.under_facility(lot.center) {
            return;
        }
        let lu = self.lots[lot_id].land_use;
        let value = self.land_value.at(self.lots[lot_id].center);
        let mut tier = Tier::for_land_value(value);
        // People waiting for somewhere affordable pull new housing down a tier.
        if lu == LandUse::Residential && self.economy.waiting_immigrants > 6 && tier > Tier::Low && self.rng.random_bool(0.5) {
            tier = tier.down();
        }
        let floors = self.pick_floors(lot_id, lu, tier);
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
        // Nothing opens overnight: the lot becomes a building site first.
        b.construction = Some(Construction::new(Construction::days_for(floors, tier)));
        b.recompute_capacity();
        let id = self.buildings.insert(b);
        self.lots[lot_id].building = Some(id);
        self.events.push(SimEvent::BuildingBuilt(id));
        self.order_materials(id);
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
            let seed = self.fresh_seed();
            let skill = Self::skill_of(seed);
            let income = job.and_then(|j| self.buildings.get(j)).map(|b| b.wage() * skill).unwrap_or(GIG_INCOME * skill);
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
            let name = names::citizen_name(&mut self.rng);
            let tick = self.clock.tick;
            let by_train = if !self.stations.is_empty() && self.rng.random_bool(0.6) { self.stations.choose(&mut self.rng).copied() } else { None };
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
                pending_footfall: Vec::new(),
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
        let skill = Self::skill_of(self.citizens[id].seed);
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
        // Tallied by `update_economy`, not every tick.
        let available = self.available;
        let population = self.citizens.len() as u32;
        let groceries_for_sale = self.groceries_for_sale;
        let meals_for_sale = self.meals_for_sale;
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
        // The destination may have been demolished on the way.
        if let Some(b) = to {
            if !self.buildings.contains_key(b) {
                let c = &mut self.citizens[id];
                c.activity = Activity::Idle;
                c.inside = None;
                c.transit = None;
                c.pending_footfall.clear();
                c.thought = "It's gone. There's nothing here any more.".to_string();
                c.next_decision_tick = tick + 1;
                return;
            }
        }
        // The journey is over (a stop is a pause in it): the shops along
        // the way count the passer-by now.
        if purpose != Purpose::ToStop {
            self.settle_footfall(id);
        }
        // A park is not a building to go inside: walk its paths to a bench.
        if let (Purpose::Visit(LandUse::Park), Some(b)) = (purpose, to) {
            let lot = self.buildings.get(b).map(|bld| bld.lot);
            let pick = self.rng.random_range(0..64usize);
            if let Some(bench) = lot.and_then(|l| self.walk.bench_in(l, pick)) {
                let here = self.citizens[id].pos;
                let path = match self.walk.route(here, bench.0) {
                    Some((pts, _)) => pts,
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
                // Venues charge at the door, and turn away anyone whose
                // savings are already through the floor.
                let cover = self.buildings.get(b).and_then(|bld| bld.business.as_ref()).filter(|bz| bz.kind == BusinessKind::Venue && !bz.closed).map(|_| VENUE_COVER);
                if cover.is_some_and(|cover| c.savings - cover < OVERDRAFT) {
                    c.activity = Activity::Idle;
                    c.inside = None;
                    c.mood = (c.mood - 0.04).max(0.0);
                    c.thought = "Turned away at the door. Skint.".to_string();
                    c.next_decision_tick = tick + 10 + self.rng.random_range(0..20);
                    return;
                }
                c.activity = Activity::Visiting(b);
                c.inside = Some(b);
                if let Some(bld) = self.buildings.get_mut(b) {
                    bld.visitors += 1;
                    let name = bld.name.clone();
                    c.remember(tick, format!("Spent the evening at {name}."));
                    c.mood = (c.mood + 0.05).min(1.0);
                    if let (Some(cover), Some(bz)) = (cover, bld.business.as_mut()) {
                        c.savings -= cover;
                        bz.money += cover;
                        bz.revenue_today += cover;
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
                // Somewhere that is open: not a site, not a shut business,
                // not a derelict shell.
                let target = self.pick_building(|b| b.land_use == lu && b.construction.is_none() && !b.is_derelict() && b.business.as_ref().map_or(true, |z| !z.closed));
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
        self.buildings.values().any(|b| b.construction.is_none() && b.business.as_ref().map_or(false, |bz| !bz.closed && bz.sells(r) && bz.stock_of(r) >= 1.0))
    }

    /// Nearest well-stocked shop for a resource, cheapest within reach.
    fn best_shop(&self, r: Resource, at: Vec2) -> Option<BuildingId> {
        let mut best: Option<(f32, BuildingId)> = None;
        for (id, b) in self.buildings.iter() {
            let Some(bz) = &b.business else { continue };
            if b.construction.is_some() || bz.closed || !bz.sells(r) || bz.stock_of(r) < 1.0 {
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
        let want: f32 = match r {
            Resource::Groceries => 3.0,
            _ => 1.0,
        };
        let price = bz.sell_price(r, jobs);
        let c = &mut self.citizens[cid];
        // Buy what the overdraft allows, whole units; nothing if not even one.
        let affordable = ((c.savings - OVERDRAFT) / price).floor();
        if affordable < 1.0 {
            c.remember(tick, format!("Couldn't afford anything at {name}."));
            c.thought = "Skint. Can't even afford the basics.".to_string();
            c.mood = (c.mood - 0.06).max(0.0);
            return;
        }
        let got = bz.take(r, want.min(affordable));
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
            Some((pts, _)) => pts,
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

    /// Stations and yards that run: connected both ways and standing.
    fn facilities(&self, kind: RailFacility) -> Vec<BuildingId> {
        self.buildings.iter().filter(|(_, b)| b.rail.as_ref().is_some_and(|l| l.kind == kind && l.works()) && !b.is_derelict()).map(|(id, _)| id).collect()
    }

    /// Recompute the working station and yard lists. Called when links,
    /// rail buildings or conditions change; everything else reads the lists.
    fn refresh_facility_cache(&mut self) {
        self.stations = self.facilities(RailFacility::Station);
        self.yards = self.facilities(RailFacility::FreightYard);
    }

    fn nearest_yard(&self, to: BuildingId) -> Option<BuildingId> {
        let p = self.building_entrance(to)?;
        self.yards.iter().copied().filter_map(|y| self.building_entrance(y).map(|q| (y, q.distance(p)))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(y, _)| y)
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
        if self.buildings.values().any(|b| self.lots.get(b.lot).is_some_and(|l| point_in_polygon(l.center, &fp) || polygons_overlap(&fp, &l.polygon))) {
            return Err("something is built there".into());
        }
        if self.facilities.values().any(|f| f.center.distance(facility.center) < Facility::size(kind).0 * 0.9) {
            return Err("too close to another station or yard".into());
        }
        self.economy.debit(Flow::Construction, cost);
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

    /// The lot a facility stands on: its footprint, fronting the nearest
    /// street (normally its own stub). `None` when the city has no street
    /// at all: a lot needs a frontage, and inventing one on the railway
    /// trunk put stations' entrances a kilometre away.
    fn facility_lot(&self, fid: FacilityId) -> Option<Lot> {
        let f = &self.facilities[fid];
        let corners = f.corners();
        let front_mid = f.front_mid();
        let probe = front_mid - f.inward() * rail::ROAD_GAP;
        let (front_segment, front_t, car_point) = self.roads.nearest_street(probe)?;
        Some(Lot {
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
        })
    }

    /// Recompute whether each station and yard reaches the railway and the
    /// city's roads.
    fn refresh_rail_links(&mut self) {
        // The railway proper is reached when a trunk end is: a facility's
        // own stub has ends too, and they do not count. The roads are
        // reached when the facility's street shares a component with a
        // zoned lot's street: there is a city at the other end of it.
        let trunk_ends: Vec<NodeId> = self.roads.rail_leaf_nodes().into_iter().filter(|&n| (self.roads.node(n).pos.y - TRUNK_Y).abs() < 1.0).collect();
        let city: std::collections::HashSet<u32> = self.lots.values().filter(|l| l.zone != ZoneId::default() && l.usable()).filter_map(|l| self.road_component(l.access_node)).collect();
        let ids: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.rail.is_some()).map(|(id, _)| id).collect();
        for id in ids {
            // The track beside the facility may have been split, re-laid or
            // removed, and segment ids are renumbered by every rebuild, so
            // find it afresh before asking whether it reaches the trunk.
            let lot_poly = self.lots.get(self.buildings[id].lot).map(|l| (l.polygon.clone(), l.center, l.front_edge));
            let kind = self.buildings[id].rail.as_ref().unwrap().kind;
            let length = if kind == RailFacility::Station { rail::STATION_PLATFORM } else { rail::YARD_APRON };
            let fresh = lot_poly.and_then(|(poly, center, front)| RailLink::beside(&self.roads, &poly, center, front, kind, length));
            let segment = fresh.as_ref().map(|f| f.segment).or_else(|| {
                let old = self.buildings[id].rail.as_ref().unwrap().segment;
                self.roads.try_segment(old).filter(|s| s.is_rail()).map(|_| old)
            });
            let rail_ok = segment.is_some_and(|sid| {
                let node = self.roads.segment(sid).a;
                trunk_ends.iter().any(|&l| self.roads.rail_path(node, l).is_some_and(|p| p.len() > 1))
            });
            let road_ok = self.lots.get(self.buildings[id].lot).and_then(|l| self.road_component(l.access_node)).is_some_and(|c| city.contains(&c));
            if let Some(mut fresh) = fresh {
                fresh.rail_connected = rail_ok;
                fresh.road_connected = road_ok;
                self.buildings[id].rail = Some(fresh);
            } else {
                let link = self.buildings[id].rail.as_mut().unwrap();
                link.rail_connected = rail_ok;
                link.road_connected = road_ok;
            }
        }
        let had_yards = !self.yards.is_empty();
        self.refresh_facility_cache();
        // The freight timetable starts when the first yard opens; otherwise
        // the stranded-goods drain would see "no train for two headways"
        // before the first one has had its chance.
        if !had_yards && !self.yards.is_empty() {
            self.rail.last_freight_tick = self.clock.tick;
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
        let name = match kind {
            RailFacility::Station => format!("{} Station", names::building_name(&mut self.rng, LandUse::Civic).split(' ').next().unwrap_or("Central")),
            RailFacility::FreightYard => format!("Yard {}", self.rng.random_range(1..30)),
        };
        let parking = if kind == RailFacility::Station { 12 } else { 4 };
        Ok(self.place_special(lot_id, name, jobs, parking, |b| b.rail = Some(link)))
    }

    /// A building the player places finished: one floor, middle tier, no
    /// homes, `jobs` jobs, no business unless `setup` adds one. Plants,
    /// depots, stations, yards and car parks all start here.
    fn place_special(&mut self, lot_id: LotId, name: String, jobs: u16, parking: u16, setup: impl FnOnce(&mut Building)) -> BuildingId {
        let lot = &self.lots[lot_id];
        let (lu, value) = (lot.land_use, self.land_value.at(lot.center));
        let seed = self.fresh_seed();
        let mut b = Building::new(lot_id, lu, 1, Tier::Mid, seed, name, self.clock.tick, value);
        b.homes = 0;
        b.jobs = jobs;
        b.business = None;
        b.parking_capacity = parking;
        setup(&mut b);
        let id = self.buildings.insert(b);
        self.lots[lot_id].building = Some(id);
        self.events.push(SimEvent::BuildingBuilt(id));
        id
    }

    /// Stations within reach of both ends of a long walk: (board, alight,
    /// where to walk to).
    fn plan_rail_trip(&self, from: Vec2, to: Vec2) -> Option<(BuildingId, BuildingId, Vec2)> {
        if from.distance(to) < rail::RAIL_TRIP_MIN {
            return None;
        }
        let stations: Vec<(BuildingId, Vec2)> = self.stations.iter().copied().filter_map(|s| self.building_entrance(s).map(|p| (s, p))).collect();
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

    /// Walk to a stop or station and wait there for the ride. `count` says
    /// whether this is the start of a journey (its footfall is noted) or
    /// the continuation of one.
    fn walk_to_transit(&mut self, id: CitizenId, stop: Vec2, leg: TransitLeg, target: BuildingId, purpose: Purpose, count: bool) {
        let exit_pos = self.leave_building(id);
        let start = exit_pos.unwrap_or(self.citizens[id].pos);
        let mut path = match self.walk.route(start, stop) {
            Some((pts, nodes)) => {
                if count {
                    self.note_footfall(id, &nodes);
                }
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
        let mut order: Vec<(BuildingId, SegmentId, Vec2)> = facilities.iter().filter_map(|&f| self.buildings.get(f)?.rail.as_ref().map(|l| (f, l.segment, l.centre()))).filter(|(_, s, _)| self.roads.try_segment(*s).is_some_and(|s| s.alive && s.is_rail())).collect();
        order.sort_by(|a, b| a.2.x.total_cmp(&b.2.x));
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
        for (i, (_, seg, centre)) in order.iter().enumerate() {
            let s = self.roads.segment(*seg);
            let pa = self.roads.node(s.a).pos;
            let pb = self.roads.node(s.b).pos;
            let here = self.roads.node(cur).pos;
            let (first, second) = if here.distance(pa) <= here.distance(pb) { (s.a, s.b) } else { (s.b, s.a) };
            if let Some(path) = self.roads.rail_path(cur, first) {
                push_path(&mut poly, &mut nodes, path, &self.roads);
                cur = first;
            }
            let (_, on_track) = crate::roads::project_on_segment(*centre, pa, pb);
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
        for (f, _, c) in &order {
            if let Some((s, d)) = rail::project_on_poly(&poly, *c) {
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
        let waiting: usize = facilities.iter().filter_map(|f| self.buildings.get(*f)).map(|b| b.rail_inbound.len()).sum();
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
        let queued = self.rail.trains.values().any(|t| t.held && t.branch.is_some_and(|(a, _)| t.s <= a + 0.5));
        let room = self.rail.trains.len() < rail::MAX_TRAINS && !queued;
        if room && Rail::in_service(minute) && !self.stations.is_empty() && tick.saturating_sub(self.rail.last_passenger_tick) >= rail::PASSENGER_HEADWAY {
            let stations = self.stations.clone();
            if self.spawn_train(TrainKind::Passenger, &stations) {
                self.rail.stats.passenger_trains_today += 1;
            }
            self.rail.last_passenger_tick = tick;
        }
        let freight_waiting = self.yards.iter().filter_map(|y| self.buildings.get(*y)).any(|b| !b.rail_inbound.is_empty() || b.rail_outbound > 0.0);
        let since_freight = tick.saturating_sub(self.rail.last_freight_tick);
        if room && !self.yards.is_empty() && since_freight >= rail::FREIGHT_HEADWAY && (freight_waiting || since_freight >= rail::FREIGHT_HEADWAY * 2) {
            let yards = self.yards.clone();
            if self.spawn_train(TrainKind::Freight, &yards) {
                self.rail.stats.freight_trains_today += 1;
            }
            self.rail.last_freight_tick = tick;
        }
        let ids: Vec<TrainId> = self.rail.trains.keys().collect();
        for id in ids {
            let blocked = self.rail.blocked(id);
            let branch_busy = self.rail.trains.iter().any(|(oid, o)| oid != id && o.on_branch());
            // A stop whose station or yard was pulled down is passed.
            while self.rail.trains[id].next_stop < self.rail.trains[id].stops.len() && !self.buildings.contains_key(self.rail.trains[id].stops[self.rail.trains[id].next_stop].0) {
                self.rail.trains[id].next_stop += 1;
            }
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
        // The counts are tallied in `update_economy`; only the countdown is
        // worth keeping to the minute.
        self.rail.stats.trains = self.rail.trains.len() as u32;
        self.rail.stats.next_passenger_in = if self.stations.is_empty() { 0 } else { rail::PASSENGER_HEADWAY.saturating_sub(tick.saturating_sub(self.rail.last_passenger_tick)) as u32 };
    }

    /// A train stands at a station or yard: people off and on, goods off.
    fn train_arrived(&mut self, id: TrainId) {
        let tick = self.clock.tick;
        let (at, kind) = {
            let t = &mut self.rail.trains[id];
            t.dwell = rail::DWELL_SECONDS;
            (t.stops[t.next_stop].0, t.kind)
        };
        let _ = tick;
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
                    self.travel_to_impl(p, Some(trip.target), trip.purpose, true, false);
                }
                let name = self.buildings.get(at).map(|b| b.name.clone()).unwrap_or_default();
                let boarding = self.board_waiting(|leg| matches!(leg, TransitLeg::Rail { board, .. } if *board == at), rail::TRAIN_CAPACITY.saturating_sub(keep.len()), rail::RAIL_FARE, Flow::RailFares, &format!("Took the train from {name}."));
                for p in boarding {
                    self.citizens[p].activity = Activity::OnTrain { train: id };
                    keep.push(p);
                    self.rail.stats.fares_today += rail::RAIL_FARE;
                    self.rail.stats.riders_today += 1;
                }
                if let Some(b) = self.buildings.get_mut(at) {
                    b.visitors = b.visitors.saturating_add(1);
                }
                self.rail.trains[id].passengers = keep;
            }
            TrainKind::Freight => {
                let Some(b) = self.buildings.get_mut(at) else { return };
                let queued = std::mem::take(&mut b.rail_inbound);
                b.rail_outbound = 0.0;
                for cargo in queued {
                    self.rail.stats.freight_in_today += cargo.amount;
                    self.dispatch_truck(TruckOrigin::Building(at), cargo, 6);
                }
            }
        }
    }

    /// People waiting at a stop or station whose leg `pred` accepts board,
    /// up to `capacity`: each pays `fare` into the treasury under `flow`
    /// and remembers `memory`. The caller seats them in its vehicle.
    fn board_waiting(&mut self, pred: impl Fn(&TransitLeg) -> bool, capacity: usize, fare: f32, flow: Flow, memory: &str) -> Vec<CitizenId> {
        let tick = self.clock.tick;
        let boarding: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::Waiting { .. }) && c.transit.as_ref().is_some_and(|t| pred(&t.leg))).map(|(id, _)| id).take(capacity).collect();
        for &p in &boarding {
            let c = &mut self.citizens[p];
            c.savings -= fare;
            c.remember(tick, memory.to_string());
            self.economy.credit(flow, fare);
        }
        boarding
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
        if self.under_facility(lot.center) {
            return Err("that ground belongs to a station or yard".into());
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
        self.economy.debit(Flow::Construction, transit::DEPOT_COST);
        let name = format!("Depot {}", self.rng.random_range(1..20));
        Ok(self.place_special(lot_id, name, transit::DEPOT_JOBS, 0, |b| b.depot = true))
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
        // Riders' trips shift with the stops; a trip through the removed
        // stop is cancelled and the rider decides again.
        let tick = self.clock.tick;
        for c in self.citizens.values_mut() {
            let Some(t) = c.transit.as_mut() else { continue };
            let TransitLeg::Bus { route: r, board, alight } = &mut t.leg else { continue };
            if *r != route {
                continue;
            }
            if *board == idx || *alight == idx {
                c.transit = None;
                if matches!(c.activity, Activity::Waiting { .. }) {
                    c.activity = Activity::Idle;
                    c.next_decision_tick = tick + 1;
                }
                continue;
            }
            if *board > idx {
                *board -= 1;
            }
            if *alight > idx {
                *alight -= 1;
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
                let depots: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.depot && !b.is_derelict()).map(|(id, _)| id).collect();
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
        // Riders who gave up: checked every ten ticks, since the limit is
        // hours and the scan is everyone.
        if tick % 10 == 0 {
            let tired: Vec<CitizenId> = self
                .citizens
                .iter()
                .filter(|(_, c)| matches!(c.activity, Activity::Waiting { .. }) && c.transit.is_some_and(|t| t.since > 0 && tick.saturating_sub(t.since) > if matches!(t.leg, TransitLeg::Rail { .. }) { transit::WAIT_GIVE_UP * 2 } else { transit::WAIT_GIVE_UP }))
                .map(|(id, _)| id)
                .collect();
            for id in tired {
                let Some(trip) = self.citizens[id].transit.take() else { continue };
                self.citizens[id].thought = "No bus is coming. Walking it.".to_string();
                self.travel_to_impl(id, Some(trip.target), trip.purpose, false, false);
            }
        }
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
            self.travel_to_impl(p, Some(trip.target), trip.purpose, false, false);
        }
        // On, up to the capacity.
        let _ = tick;
        let memory = format!("Caught the {} bus.", self.transit.routes[bus.route].name);
        let boarding = self.board_waiting(|leg| matches!(leg, TransitLeg::Bus { route, board, .. } if *route == bus.route && *board == stop_idx), transit::BUS_CAPACITY.saturating_sub(passengers.len()), fare, Flow::BusFares, &memory);
        for p in boarding {
            self.citizens[p].activity = Activity::Riding { bus: car_id };
            passengers.push(p);
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
        if self.under_facility(lot.center) {
            return Err("that ground belongs to a station or yard".into());
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
        self.economy.debit(Flow::Construction, kind.build_cost());
        let name = format!("{} {}", match kind {
            PowerPlantKind::Coal => "Blackridge",
            PowerPlantKind::Solar => "Sunfield",
            PowerPlantKind::Wind => "Westerly",
            PowerPlantKind::Nuclear => "Calder",
        }, self.rng.random_range(1..9));
        let id = self.place_special(lot_id, name, kind.jobs(), kind.jobs() / 2, |b| b.plant = Some(kind));
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
        if lot.building.is_some() || lot.landlocked || self.under_facility(lot.center) {
            return None;
        }
        let area = lot.area();
        let value = self.land_value.at(lot.center);
        let tier = Tier::for_land_value(value).min(Tier::Mid);
        let floors: u8 = if tier == Tier::Mid && area > 500.0 { self.rng.random_range(3..=5) } else { 0 };
        let capacity = if floors == 0 { (area / 28.0) as u16 } else { ((area / 32.0) as u16) * floors as u16 };
        if capacity < 4 {
            return None;
        }
        let name = if floors == 0 { format!("Lot {}", self.rng.random_range(1..99)) } else { format!("ParkHaus {}", self.rng.random_range(1..40)) };
        let jobs = if floors == 0 { 2 } else { 4 };
        let mut bz = Business::new(BusinessKind::Parking, jobs);
        bz.money = 80.0;
        Some(self.place_special(lot_id, name, jobs, capacity, |b| {
            // A surface lot is no building at all; a ParkHaus is a cheap one.
            b.tier = tier;
            b.floors = floors;
            b.business = Some(bz);
            b.parking_price = 3.0 * tier.wage_multiplier() * (0.6 + value);
        }))
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
        self.travel_to_impl(id, target, purpose, true, true);
    }

    /// Start a journey: on foot, by bike, by car, or by bus or train with a
    /// walk at each end. `fresh` is false for the legs after a ride, so a
    /// journey's footfall is noted once, for the leg that began it.
    fn travel_to_impl(&mut self, id: CitizenId, target: Option<BuildingId>, purpose: Purpose, allow_bus: bool, fresh: bool) {
        let Some(target) = target else { return };
        self.citizens[id].transit = None;
        if fresh {
            self.citizens[id].pending_footfall.clear();
        }
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
            if let Some(exit) = self.leave_building(id) {
                self.citizens[id].pos = exit;
            }
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
                self.walk_to_transit(id, stop, TransitLeg::Rail { board, alight }, target, purpose, fresh);
                return;
            }
        }
        // The bus, for long trips on foot when a line serves both ends and
        // a depot has buses out. Walk to the stop first.
        if allow_bus && mode == TravelMode::Walk && self.economy.transit.buses > 0 {
            if let Some((route, board, alight)) = self.transit.plan_trip(walk_from, entrance) {
                let stop = self.transit.routes[route].stops[board].pos;
                self.walk_to_transit(id, stop, TransitLeg::Bus { route, board, alight }, target, purpose, fresh);
                return;
            }
        }
        let exit_pos = self.leave_building(id);
        let mut path = match mode {
            TravelMode::Walk => match self.walk.route(walk_from, entrance) {
                Some((pts, walk_nodes)) => {
                    if fresh {
                        self.note_footfall(id, &walk_nodes);
                    }
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

    /// Remember which lots a journey on foot will pass; they are counted
    /// when the walker gets there.
    fn note_footfall(&mut self, id: CitizenId, walk_nodes: &[u32]) {
        let lots = self.walk.lots_passed(walk_nodes);
        if let Some(c) = self.citizens.get_mut(id) {
            c.pending_footfall = lots;
        }
    }

    /// Everyone on foot who passes a lot is a potential customer: credit
    /// the lots of the journey just completed, once.
    fn settle_footfall(&mut self, id: CitizenId) {
        let Some(c) = self.citizens.get_mut(id) else { return };
        let lots = std::mem::take(&mut c.pending_footfall);
        if lots.is_empty() {
            return;
        }
        self.economy.walk_trips_today += 1;
        for lid in lots {
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
                self.note_footfall(id, &walk_nodes);
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
        let c = &mut self.citizens[id];
        c.mode = mode;
        c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
    }
}

/// Cover charge at a venue's door.
const VENUE_COVER: f32 = 15.0;

/// Where a truck sets off from.
#[derive(Clone, Copy, Debug)]
enum TruckOrigin {
    Building(BuildingId),
    /// The far end of a road leaving the map: an import.
    CityEdge,
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

    /// Open every building site at once, for tests about what happens after.
    fn finish_all(w: &mut World) {
        let ids: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
        for id in ids {
            w.finish_construction(id);
        }
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
        finish_all(&mut w);
        for _ in 0..120 {
            w.tick(&mut minds);
        }
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
        finish_all(&mut w);
        for _ in 0..60 {
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
        w.finish_construction(shop);
        let lot = w.build_parking(lots[1]).expect("parking lot opens");
        assert!(w.buildings[lot].construction.is_none(), "parking is laid out at once");
        let cap = w.buildings[lot].parking_capacity;
        assert!(cap >= 4 && w.buildings[lot].is_parking() && w.buildings[lot].jobs == 2);
        assert!(w.buildings[lot].parking_price > 0.0);
        // Low-tier shops have no spaces of their own, so a driver pays at the lot.
        let mut minds = IdleMind;
        for _ in 0..(12 * 60) {
            w.tick(&mut minds);
        }
        finish_all(&mut w);
        for _ in 0..60 {
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
        w.finish_construction(home);
        w.finish_construction(park);
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
        w.finish_construction(home);
        w.finish_construction(shop);
        for _ in 0..(6 * 60) {
            w.tick(&mut minds);
        }
        let c = w.citizens.keys().next().expect("someone arrived");
        // Buses sleep at the depot overnight: start the trip mid-morning
        // with a bus out on the line. A lap of the block takes longer than
        // a rider will wait, so set off as the bus heads for the stop
        // before the rider's.
        let bus_due = |w: &World| w.traffic.cars.values().any(|c| c.bus.as_ref().is_some_and(|b| b.next_stop == 4 && !b.to_depot));
        while !(7 * 60..=12 * 60).contains(&(w.clock.minute_of_day() as u32)) || w.economy.transit.buses == 0 || !bus_due(&w) {
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
        w.finish_construction(home);
        w.finish_construction(shop);
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
        finish_all(&mut w);
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

    /// The usable lot of `lu` nearest `p`, vacant.
    fn lot_near(w: &World, p: Vec2, lu: LandUse) -> LotId {
        w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == lu).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|(id, _)| id).expect("a vacant lot")
    }

    #[test]
    fn road_works_pull_down_the_buildings_whose_parcels_change() {
        let mut w = World::new(WorldConfig::default());
        let road = w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(-400.0, 400.0), Vec2::new(400.0, 400.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        w.add_zone(square(10.0, 410.0, 190.0), LandUse::Residential);
        let front_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
        assert!(w.lots[front_lot].front_segment == w.roads.nearest_street(Vec2::new(100.0, 20.0)).unwrap().0, "the lot fronts the main road");
        w.build_on(front_lot);
        let fronting = w.lots[front_lot].building.unwrap();
        let far_lot = lot_near(&w, Vec2::new(100.0, 420.0), LandUse::Residential);
        w.build_on(far_lot);
        finish_all(&mut w);
        let far = w.lots[far_lot].building.unwrap();
        // People move in, so somebody is there to be displaced.
        let mut minds = IdleMind;
        for _ in 0..400 {
            w.tick(&mut minds);
        }
        assert!(!w.buildings[fronting].residents.is_empty(), "residents moved in");
        let resident = w.buildings[fronting].residents[0];
        let old_outline = w.lots[front_lot].polygon.clone();
        w.drain_events();

        // Widen the road to 3+3: every parcel along it loses a strip.
        w.set_gesture(road, Gesture::Road { points: vec![Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], lanes_fwd: 3, lanes_bwd: 3, curved: false, kind: RoadKind::Street }, true);
        w.implement();
        assert!(!w.buildings.contains_key(fronting), "the building fronting the widened road is gone");
        assert!(w.buildings.contains_key(far), "a building on an untouched lot stays");
        assert!(w.lots.values().all(|l| l.building != Some(fronting)));
        let events = w.drain_events();
        assert!(events.iter().any(|e| matches!(e, SimEvent::Expropriated(b, _) if *b == fronting)), "an expropriation event was raised: {events:?}");
        assert!(events.iter().any(|e| matches!(e, SimEvent::BuildingRemoved(b, _) if *b == fronting)));
        let c = &w.citizens[resident];
        assert_eq!(c.home, None);
        assert!(c.memories.iter().any(|m| m.text.contains("pulled down for the road works")), "{:?}", c.memories);
        // The ground is still there, as a smaller lot, and can be built on again.
        let new_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
        assert!(outlines_differ(&old_outline, &w.lots[new_lot].polygon, EXPROPRIATE_AREA_TOL, EXPROPRIATE_DIST_TOL));

        // A footpath drawn through a built lot removes that building too.
        let far_center = w.lots[far_lot].center;
        w.add_path(&[Vec2::new(far_center.x, 380.0), Vec2::new(far_center.x, 620.0)]);
        assert!(!w.buildings.contains_key(far), "a path through the lot takes the building");
        assert!(w.lots.values().all(|l| l.building != Some(far)));
    }

    #[test]
    fn unchanged_parcels_keep_their_buildings_through_unrelated_works() {
        // A new road elsewhere must not shake buildings off parcels that did
        // not change: the outline comparison has to tolerate regeneration.
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
        for &l in &lots {
            w.build_on(l);
        }
        let n = w.buildings.len();
        assert!(n >= 3);
        w.add_road(&[Vec2::new(-300.0, -300.0), Vec2::new(-300.0, 300.0)], 2, 2);
        assert_eq!(w.buildings.len(), n, "nothing fronting the untouched road was expropriated");
        assert!(w.drain_events().iter().all(|e| !matches!(e, SimEvent::Expropriated(..))));
    }

    #[test]
    fn stations_and_yards_never_share_ground_with_other_buildings() {
        use crate::rail::RailFacility;
        let mut w = World::new(WorldConfig::default());
        w.economy.treasury = 500_000.0;
        w.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(1000.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(150.0, 10.0), Vec2::new(330.0, 10.0), Vec2::new(330.0, 90.0), Vec2::new(150.0, 90.0)], LandUse::Commercial);
        w.add_zone(vec![Vec2::new(650.0, 10.0), Vec2::new(830.0, 10.0), Vec2::new(830.0, 90.0), Vec2::new(650.0, 90.0)], LandUse::Commercial);
        // A shop stands where the station would go: the placement is refused.
        let shop_lot = lot_near(&w, Vec2::new(240.0, 25.0), LandUse::Commercial);
        w.build_on(shop_lot);
        let shop = w.lots[shop_lot].building.unwrap();
        let shop_center = w.lots[shop_lot].center;
        let err = w.place_rail_facility(RailFacility::Station, shop_center).expect_err("a station cannot go over a shop");
        assert!(err.contains("built"), "{err}");
        assert!(w.buildings.contains_key(shop) && w.facilities.is_empty());
        let lots_before = w.lots.len();
        // A yard over vacant zoned lots takes the ground; the lots under and
        // across its footprint give way.
        let yard = w.place_rail_facility(RailFacility::FreightYard, Vec2::new(740.0, 40.0)).expect("yard");
        assert!(w.lots.len() < lots_before, "zoned lots under the yard gave way");
        let fp = w.facilities.values().next().unwrap().footprint();
        let yard_lot = w.buildings[yard].lot;
        for (id, l) in w.lots.iter() {
            if id == yard_lot {
                continue;
            }
            assert!(!point_in_polygon(l.center, &fp), "a lot centre under the yard: {:?}", l.center);
            assert!(!polygons_overlap(&fp, &l.polygon), "a lot crossing the yard: {:?}", l.polygon);
        }
        // Nothing ever gets built on that ground: force development onto
        // every lot and let the city develop on its own for a while.
        let all: Vec<LotId> = w.lots.keys().collect();
        for l in all {
            w.build_on(l);
        }
        w.economy.demand = [1.0; 6];
        for _ in 0..50 {
            w.develop();
        }
        for (id, b) in w.buildings.iter() {
            if id == yard {
                continue;
            }
            let l = &w.lots[b.lot];
            assert!(!w.under_facility(l.center), "{} was built under the yard", b.name);
            assert!(!polygons_overlap(&fp, &l.polygon), "{} shares ground with the yard", b.name);
        }
        // Plants, depots and parking refuse facility ground too (the yard's
        // own lot is the only lot there, and it is taken).
        assert!(w.build_power(yard_lot, crate::power::PowerPlantKind::Solar).is_err());
        assert!(w.build_depot(yard_lot).is_err());
        assert!(w.build_parking(yard_lot).is_none());
        assert!(w.buildings.contains_key(yard) && w.buildings[yard].rail.is_some());
    }

    #[test]
    fn cheap_buildings_on_dear_land_are_redeveloped_and_price_people_out() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        w.land_value.fill(0.1);
        let lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
        w.build_on(lot);
        finish_all(&mut w);
        let bid = w.lots[lot].building.unwrap();
        assert_eq!(w.buildings[bid].tier, Tier::Low);
        let rent0 = w.buildings[bid].rent;
        // Tenants move in while the land is cheap. The grid is recomputed
        // every two hours, so the scenario value is pinned before each tick.
        // Nobody here eats (IdleMind), so satiety is pinned: the test is
        // about rent driving people out, not hunger.
        let fed = |w: &mut World| {
            for c in w.citizens.values_mut() {
                c.satiety = 1.0;
            }
        };
        let mut minds = IdleMind;
        for _ in 0..600 {
            w.land_value.fill(0.1);
            fed(&mut w);
            w.tick(&mut minds);
        }
        let residents = w.buildings[bid].residents.clone();
        assert!(residents.len() >= 2, "tenants moved in: {}", residents.len());
        assert_eq!(w.buildings[bid].tier, Tier::Low, "cheap land keeps a cheap building");
        // The land becomes dear. Three days of that and the building is rebuilt.
        for _ in 0..(REDEVELOP_DAYS as usize + 1) * 1440 {
            w.land_value.fill(0.95);
            fed(&mut w);
            w.tick(&mut minds);
        }
        let b = &w.buildings[bid];
        assert!(b.tier > Tier::Low, "the building left the low tier: {:?}", b.tier);
        assert!(b.rent > rent0 * 2.0, "rent rose from {rent0:.0} to {:.0}", b.rent);
        assert!(b.residents.iter().all(|c| residents.contains(c)), "no one new moved in yet");
        assert!(w.citizens.values().any(|c| c.memories.iter().any(|m| m.text.contains("rebuilt for richer tenants"))));
        // The old tenants cannot pay the new rent and move on.
        let rent = w.buildings[bid].rent;
        let priced_out: Vec<CitizenId> = w.buildings[bid].residents.iter().copied().filter(|&c| w.citizens[c].income * 0.5 < rent).collect();
        assert!(!priced_out.is_empty(), "somebody is priced out by {rent:.0}/day");
        for _ in 0..5 * 1440 {
            w.land_value.fill(0.95);
            fed(&mut w);
            w.tick(&mut minds);
        }
        assert!(w.citizens.values().all(|c| c.hungry_days == 0), "nobody is starving: what follows is about rent");
        for cid in &priced_out {
            assert!(w.citizens.get(*cid).map_or(true, |c| c.home != Some(bid)), "a priced-out tenant is still there");
        }
        assert!(w.economy.displaced_total + w.economy.emigrated_total > 0);
    }

    #[test]
    fn heights_follow_density() {
        let mut w = World::new(WorldConfig::default());
        // One city block ringed by streets.
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(-400.0, 240.0), Vec2::new(400.0, 240.0)], 1, 1);
        w.add_road(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 340.0)], 1, 1);
        w.add_road(&[Vec2::new(240.0, -100.0), Vec2::new(240.0, 340.0)], 1, 1);
        w.add_zone(square(0.0, 0.0, 240.0), LandUse::Residential);
        // Dear land: a tower zone, but nothing stands yet.
        w.land_value.fill(0.95);
        let middle = Vec2::new(120.0, 120.0);
        let mut lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, _)| id).collect();
        lots.sort_by(|a, b| w.lots[*a].center.distance(middle).total_cmp(&w.lots[*b].center.distance(middle)));
        assert!(lots.len() >= 16, "{} lots", lots.len());
        assert_eq!(w.height_cap(lots[0]), 2, "an empty field allows two floors");
        w.build_on(lots[0]);
        let first = w.buildings[w.lots[lots[0]].building.unwrap()].clone();
        assert_eq!(first.tier, Tier::High);
        assert!(first.floors >= 1 && first.floors <= 3, "the first building is short: {} floors", first.floors);
        // A few more go up around it; they stay short while the block is sparse.
        for &l in &lots[1..6] {
            w.build_on(l);
        }
        assert!(w.buildings.values().all(|b| b.floors <= 4), "{:?}", w.buildings.values().map(|b| b.floors).collect::<Vec<_>>());
        // Once the block is dense (every neighbour a tower), the full range opens.
        for b in w.buildings.values_mut() {
            b.floors = 40;
        }
        assert!(w.height_cap(lots[6]) >= 12, "cap {}", w.height_cap(lots[6]));
        let mut tallest = 0;
        for &l in &lots[6..12] {
            w.build_on(l);
            tallest = tallest.max(w.buildings[w.lots[l].building.unwrap()].floors);
        }
        assert!(tallest > 6, "a new building in a dense block can be tall: {tallest}");
        // Parks are untouched by the rule.
        let mut p = World::new(WorldConfig::default());
        p.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        p.add_zone(square(10.0, 10.0, 100.0), LandUse::Park);
        let park = lot_near(&p, Vec2::new(60.0, 20.0), LandUse::Park);
        p.build_on(park);
        assert_eq!(p.buildings[p.lots[park].building.unwrap()].floors, 0);
    }

    #[test]
    fn buildings_go_up_over_days_and_people_move_in_after() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        let lot = w.lots.iter().find(|(_, l)| l.usable()).map(|(id, _)| id).unwrap();
        w.build_on(lot);
        let id = w.lots[lot].building.unwrap();
        let (floors, tier) = (w.buildings[id].floors, w.buildings[id].tier);
        {
            let b = &w.buildings[id];
            let c = b.construction.as_ref().expect("a building site, not a building");
            assert_eq!(c.progress, 0.0);
            assert!((c.days - Construction::days_for(floors, tier)).abs() < 1e-6 && c.days >= 1.0);
            assert_eq!(b.homes, 0, "nobody can live on a site");
            assert_eq!(b.jobs, Construction::crew(floors), "the crew is the only work here");
            assert!(b.wage() > 0.0, "labourers are paid");
            assert_eq!(b.parking_capacity, 0);
        }
        let (planned_homes, _) = w.buildings[id].planned_capacity();
        assert!(planned_homes > 0);
        // The first load was ordered right away: a truck is on the road, or
        // it was dropped on the spot when no route exists.
        {
            let c = w.buildings[id].construction.as_ref().unwrap();
            assert!(c.deliveries == 1 || c.ordered, "{c:?}");
            assert!(w.economy.materials_today > 0.0, "parts are bought from outside when no fab is in town");
        }
        // The site is still standing after a save.
        let back = World::from_ron(&w.to_ron().unwrap()).unwrap();
        assert_eq!(back.buildings[id].construction, w.buildings[id].construction);

        let mut minds = IdleMind;
        let mut changed = 0usize;
        let mut hours = 0u32;
        w.drain_events();
        while w.buildings[id].construction.is_some() {
            for _ in 0..60 {
                w.tick(&mut minds);
            }
            hours += 1;
            changed += w.drain_events().iter().filter(|e| matches!(e, SimEvent::BuildingChanged(x) if *x == id)).count();
            let b = &w.buildings[id];
            if b.construction.is_some() {
                assert!(b.residents.is_empty() && b.homes == 0, "nobody moves in before the building opens");
            }
            assert!(w.economy.sites >= 1 || hours < 1);
            assert!(hours < 24 * 20, "construction never finished: {:?}", b.construction);
        }
        let days = Construction::days_for(floors, tier);
        assert!(hours as f32 >= days * 24.0 - 1.0, "finished in {hours} h, planned {days} days");
        assert!(hours as f32 <= days * 24.0 + 8.0, "took {hours} h for {days} days of work: stalled too long");
        assert!(changed >= 9, "the renderer saw the site grow: {changed} redraws");
        let b = &w.buildings[id];
        assert!(b.homes > 0 && b.homes == planned_homes, "homes {} planned {planned_homes}", b.homes);
        assert_eq!(b.jobs, 0);
        assert!(b.workers.is_empty(), "the crew was let go");
        // People move in once it is open.
        for _ in 0..240 {
            w.tick(&mut minds);
            if !w.buildings[id].residents.is_empty() {
                break;
            }
        }
        assert!(!w.buildings[id].residents.is_empty(), "residents move in after completion");
    }

    #[test]
    fn a_site_waits_for_its_materials_and_resumes_on_delivery() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Commercial);
        let lot = w.lots.iter().find(|(_, l)| l.usable()).map(|(id, _)| id).unwrap();
        w.build_on(lot);
        let id = w.lots[lot].building.unwrap();
        let mut minds = IdleMind;
        // Let the first truck arrive, then pretend it never did: nothing happens.
        for _ in 0..600 {
            if w.buildings[id].construction.as_ref().unwrap().deliveries >= 1 {
                break;
            }
            w.tick(&mut minds);
        }
        let p0 = {
            let c = w.buildings[id].construction.as_mut().unwrap();
            assert_eq!(c.deliveries, 1, "the first load came: {c:?}");
            c.deliveries = 0;
            c.ordered = true;
            assert!(c.waiting_for_materials());
            c.progress
        };
        for _ in 0..180 {
            w.tick(&mut minds);
        }
        assert_eq!(w.buildings[id].construction.as_ref().unwrap().progress, p0, "no parts, no work");
        assert_eq!(w.buildings[id].construction.as_ref().unwrap().deliveries, 0, "no second order while one is on its way");
        // The truck arrives.
        w.deliver(Cargo { from: id, to: id, resource: Resource::Parts, amount: 8.0, price: 0.0 });
        {
            let c = w.buildings[id].construction.as_ref().unwrap();
            assert_eq!(c.deliveries, 1);
            assert!(!c.ordered && !c.waiting_for_materials());
        }
        for _ in 0..60 {
            w.tick(&mut minds);
        }
        let p = w.buildings[id].construction.as_ref().unwrap().progress;
        assert!(p > p0, "work resumed: {p}");
        // Work stops again at the quarter until the next load comes (the
        // one ordered ahead is still on the road).
        {
            let c = w.buildings[id].construction.as_mut().unwrap();
            c.progress = 0.25 - c.hourly_step() * 0.5;
            c.deliveries = 1;
            c.ordered = true;
        }
        for _ in 0..240 {
            w.tick(&mut minds);
        }
        let c = w.buildings[id].construction.as_ref().unwrap();
        assert!((c.progress - 0.25).abs() < 1e-6, "stalls at the quarter: {}", c.progress);
        assert!(c.waiting_for_materials());
        w.deliver(Cargo { from: id, to: id, resource: Resource::Parts, amount: 8.0, price: 0.0 });
        for _ in 0..60 {
            w.tick(&mut minds);
        }
        assert!(w.buildings[id].construction.as_ref().unwrap().progress > 0.25);
        // A delivery to a site never lands on the shop's shelves.
        let stock: f32 = w.buildings[id].business.as_ref().map_or(0.0, |bz| bz.stock_of(Resource::Parts));
        assert_eq!(stock, 0.0);
        // Skipping the wait opens the building with its real jobs.
        let planned = w.buildings[id].planned_capacity();
        w.finish_construction(id);
        let b = &w.buildings[id];
        assert!(b.construction.is_none());
        assert_eq!((b.homes, b.jobs), planned);
        assert!(b.jobs > 0 && b.jobs != Construction::crew(b.floors));
    }

    #[test]
    fn site_crews_commute_and_are_released_when_the_building_opens() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
        let home_lot = w.lots.iter().find(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, _)| id).unwrap();
        let site_lot = w.lots.iter().find(|(_, l)| l.usable() && l.land_use == LandUse::Commercial).map(|(id, _)| id).unwrap();
        w.build_on(home_lot);
        let home = w.lots[home_lot].building.unwrap();
        w.finish_construction(home);
        w.build_on(site_lot);
        let site = w.lots[site_lot].building.unwrap();
        // Make the site the only employer so the newcomers join the crew.
        let others: Vec<BuildingId> = w.buildings.keys().filter(|&b| b != home && b != site).collect();
        for b in others {
            w.buildings[b].jobs = 0;
        }
        let mut minds = IdleMind;
        for _ in 0..(4 * 60) {
            w.tick(&mut minds);
            for b in w.buildings.values_mut() {
                if b.construction.is_some() && b.lot != site_lot {
                    b.jobs = 0;
                }
            }
        }
        assert!(!w.buildings[site].workers.is_empty(), "the unemployed take site work");
        let crew = w.buildings[site].workers.clone();
        for &c in &crew {
            assert_eq!(w.citizens[c].work, Some(site));
            assert!((w.citizens[c].income - Building::SITE_WAGE * (0.75 + (w.citizens[c].seed % 1000) as f32 / 1000.0 * 0.6)).abs() < 0.01);
        }
        w.finish_construction(site);
        assert!(w.buildings[site].workers.is_empty());
        for &c in &crew {
            assert_eq!(w.citizens[c].work, None, "laid off cleanly");
            assert_ne!(w.citizens[c].inside, Some(site));
        }
    }

    #[test]
    fn player_placed_plants_and_depots_work_from_the_moment_of_placement() {
        use crate::power::PowerPlantKind;
        use crate::transit;
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
        let ind: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial).map(|(id, _)| id).collect();
        let big = ind.iter().copied().max_by(|a, b| w.lots[*a].area().total_cmp(&w.lots[*b].area())).unwrap();
        let plant = w.build_power(big, PowerPlantKind::Coal).expect("coal plant");
        let b = &w.buildings[plant];
        assert!(b.construction.is_none(), "specials are placed finished");
        assert_eq!(b.jobs, PowerPlantKind::Coal.jobs());
        assert_eq!(w.economy.power.plants, 1, "it is on the grid at once");
        let depot_lot = ind.iter().copied().find(|&l| l != big && w.lots[l].building.is_none() && w.lots[l].area() >= transit::DEPOT_MIN_AREA && w.lots[l].car_access());
        if let Some(l) = depot_lot {
            let depot = w.build_depot(l).expect("depot");
            assert!(w.buildings[depot].construction.is_none() && w.buildings[depot].jobs == transit::DEPOT_JOBS);
        }
    }

    #[test]
    fn bulldozing_the_track_under_a_station_disconnects_it_without_panicking() {
        use crate::rail::RailFacility;
        let mut w = World::new(WorldConfig::default());
        w.economy.treasury = 200_000.0;
        let branch = w.add_rail(&[Vec2::new(-200.0, TRUNK_Y), Vec2::new(-100.0, -400.0), Vec2::new(-40.0, -300.0), Vec2::new(0.0, -200.0), Vec2::new(0.0, -60.0), Vec2::new(900.0, -60.0)]);
        w.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(1000.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(150.0, 10.0), Vec2::new(330.0, 10.0), Vec2::new(330.0, 90.0), Vec2::new(150.0, 90.0)], LandUse::Residential);
        let station = w.place_rail_facility(RailFacility::Station, Vec2::new(240.0, -45.0)).expect("station");
        assert!(w.buildings[station].rail.as_ref().unwrap().rail_connected);
        // Segment ids are renumbered by the rebuild; the old one must not be trusted.
        w.start_project();
        w.remove_gesture(branch);
        w.implement();
        let link = w.buildings.get(station).and_then(|b| b.rail.clone());
        assert!(link.as_ref().is_some_and(|l| !l.rail_connected), "the station stands but is off the railway: {link:?}");
        let mut minds = crate::mind::IdleMind;
        for _ in 0..600 {
            w.tick(&mut minds);
        }
        assert!(w.rail.trains.values().all(|t| t.stops.iter().all(|(s, _)| w.buildings.contains_key(*s))), "no train aims at a vanished stop");
    }

    // ── review follow-ups ────────────────────────────────────────────────────

    /// The rail scenario of `trains_call_at_stations_and_yards_bring_freight`
    /// up to the point where trains run: branch, street, zones, two
    /// connected stations and a connected yard.
    fn rail_city() -> (World, GestureId, BuildingId, BuildingId, BuildingId) {
        use crate::rail::RailFacility;
        let mut w = World::new(WorldConfig::default());
        w.economy.treasury = 200_000.0;
        let branch = w.add_rail(&[Vec2::new(-200.0, TRUNK_Y), Vec2::new(-100.0, -400.0), Vec2::new(-40.0, -300.0), Vec2::new(0.0, -200.0), Vec2::new(0.0, -60.0), Vec2::new(900.0, -60.0)]);
        w.add_road(&[Vec2::new(-100.0, 0.0), Vec2::new(1000.0, 0.0)], 1, 1);
        w.add_zone(vec![Vec2::new(150.0, 10.0), Vec2::new(330.0, 10.0), Vec2::new(330.0, 90.0), Vec2::new(150.0, 90.0)], LandUse::Residential);
        w.add_zone(vec![Vec2::new(650.0, 10.0), Vec2::new(830.0, 10.0), Vec2::new(830.0, 90.0), Vec2::new(650.0, 90.0)], LandUse::Commercial);
        let place = |w: &mut World, kind, p| {
            let id = w.place_rail_facility(kind, p).expect("facility");
            let end = w.facilities.values().find(|f| f.building == Some(id)).unwrap().road_line()[1];
            w.add_road(&[end, Vec2::new(end.x, 0.0)], 1, 1);
            assert!(w.buildings[id].rail.as_ref().unwrap().works(), "{:?}", w.buildings[id].rail);
            id
        };
        let a = place(&mut w, RailFacility::Station, Vec2::new(240.0, -45.0));
        let b = place(&mut w, RailFacility::Station, Vec2::new(740.0, -45.0));
        let yard = place(&mut w, RailFacility::FreightYard, Vec2::new(460.0, -50.0));
        (w, branch, a, b, yard)
    }

    #[test]
    fn bulldozing_the_branch_takes_the_trains_off_and_strands_riders_sanely() {
        use crate::rail::TrainKind;
        let (mut w, branch, station_a, station_b, _) = rail_city();
        let home_lot = lot_near(&w, Vec2::new(240.0, 40.0), LandUse::Residential);
        w.build_on(home_lot);
        let home = w.lots[home_lot].building.unwrap();
        w.finish_construction(home);
        let mut minds = IdleMind;
        for _ in 0..600 {
            if !w.citizens.is_empty() {
                break;
            }
            w.tick(&mut minds);
        }
        let rider = w.citizens.keys().next().expect("someone moved in");
        // Let a passenger train get well onto the branch.
        let mut train = None;
        for _ in 0..600 {
            w.tick(&mut minds);
            if let Some((id, t)) = w.rail.trains.iter().find(|(_, t)| t.kind == TrainKind::Passenger && t.on_branch()) {
                train = Some((id, t.head()));
                break;
            }
        }
        let (train, head) = train.expect("a train on the branch");
        // Someone is aboard, bound for station B.
        w.rail.trains[train].passengers.push(rider);
        w.citizens[rider].activity = Activity::OnTrain { train };
        w.citizens[rider].transit = Some(TransitTrip { leg: TransitLeg::Rail { board: station_a, alight: station_b }, target: home, purpose: Purpose::Home, since: 0 });
        w.start_project();
        w.remove_gesture(branch);
        w.implement();
        assert!(w.rail.trains.is_empty(), "every train came off with the rebuild");
        let c = &w.citizens[rider];
        assert!(matches!(c.activity, Activity::Idle) && c.transit.is_none(), "the rider stands where the train was: {}", c.activity.label());
        assert!(c.pos.distance(head) < 1.0 && c.pos.is_finite());
        assert!(!w.buildings[station_a].rail.as_ref().unwrap().rail_connected, "the stations are off the railway");
        for _ in 0..600 {
            w.tick(&mut minds);
            for t in w.rail.trains.values() {
                assert!(t.stops.iter().all(|(s, _)| w.buildings.contains_key(*s)), "no train aims at a vanished stop");
                for p in &t.poly {
                    let (_, _, q) = w.roads.nearest_rail(*p).expect("track exists");
                    assert!(q.distance(*p) < 3.0, "a train's route leaves the track at {p:?}");
                }
            }
        }
        assert!(w.citizens.values().all(|c| !matches!(c.activity, Activity::OnTrain { .. }) || w.rail.trains.values().any(|t| t.passengers.contains(&rider))));
    }

    #[test]
    fn imports_stranded_at_a_disconnected_yard_go_by_truck_and_sites_never_wait() {
        let (mut w, branch, _, _, yard) = rail_city();
        let shop_lot = lot_near(&w, Vec2::new(815.0, 40.0), LandUse::Commercial);
        w.build_on(shop_lot);
        let shop = w.lots[shop_lot].building.unwrap();
        w.finish_construction(shop);
        // A site's parts never queue for a train.
        let queued0 = w.buildings[yard].rail_inbound.len();
        let site_lot = lot_near(&w, Vec2::new(700.0, 40.0), LandUse::Commercial);
        w.build_on(site_lot);
        assert_eq!(w.buildings[yard].rail_inbound.len(), queued0, "construction parts came by truck");
        // A shop's order does, and it is stranded when the branch goes.
        w.import(shop, Resource::Parts, 10.0, 100.0);
        assert_eq!(w.buildings[yard].rail_inbound.len(), queued0 + 1);
        let stock0 = w.buildings[shop].business.as_ref().unwrap().stock_of(Resource::Parts);
        w.start_project();
        w.remove_gesture(branch);
        w.implement();
        assert!(!w.buildings[yard].rail.as_ref().unwrap().works());
        let mut minds = IdleMind;
        for _ in 0..60 {
            w.tick(&mut minds);
        }
        assert!(w.buildings[yard].rail_inbound.is_empty(), "the stranded order left the yard within the hour");
        let delivered = w.buildings[shop].business.as_ref().unwrap().stock_of(Resource::Parts) > stock0;
        let on_the_road = w.traffic.cars.values().any(|c| c.cargo.is_some_and(|k| k.to == shop && k.resource == Resource::Parts));
        assert!(delivered || on_the_road, "the parts are on a truck or on the shelf");
        // A working yard whose trains stopped coming is drained too.
        let (mut w, _, _, _, yard) = rail_city();
        let shop_lot = lot_near(&w, Vec2::new(815.0, 40.0), LandUse::Commercial);
        w.build_on(shop_lot);
        let shop = w.lots[shop_lot].building.unwrap();
        w.finish_construction(shop);
        while w.clock.tick % 60 != 59 {
            w.tick(&mut minds);
        }
        w.import(shop, Resource::Parts, 10.0, 100.0);
        assert_eq!(w.buildings[yard].rail_inbound.len(), 1);
        w.rail.last_freight_tick = 0;
        w.tick(&mut minds);
        assert!(w.buildings[yard].rail_inbound.is_empty(), "two headways without a freight train and the goods go by road");
    }

    #[test]
    fn a_short_commute_at_eight_arrives_before_nine() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
        let home_lot = lot_near(&w, Vec2::new(150.0, 20.0), LandUse::Residential);
        let work_lot = lot_near(&w, Vec2::new(-150.0, 20.0), LandUse::Commercial);
        w.build_on(home_lot);
        w.build_on(work_lot);
        let home = w.lots[home_lot].building.unwrap();
        let work = w.lots[work_lot].building.unwrap();
        w.finish_construction(home);
        w.finish_construction(work);
        let apart = w.lots[home_lot].entrance().distance(w.lots[work_lot].entrance());
        assert!((250.0..=350.0).contains(&apart), "home and work are {apart:.0} m apart");
        assert!((w.config.walk_speed - w.config.metres_per_tick(WALK_MPS)).abs() < 1e-6);
        assert!((w.config.bike_speed - BIKE_MPS * w.config.traffic_seconds_per_tick).abs() < 1e-6);
        let mut minds = IdleMind;
        for _ in 0..600 {
            if !w.citizens.is_empty() {
                break;
            }
            w.tick(&mut minds);
        }
        let c = w.citizens.keys().next().expect("someone moved in");
        {
            let cz = &mut w.citizens[c];
            cz.inside = Some(home);
            cz.activity = Activity::AtHome;
            cz.owns_bike = false;
            cz.owns_car = false;
            cz.work = Some(work);
        }
        w.clock.tick = (w.clock.day() + 1) * crate::clock::MINUTES_PER_DAY + 8 * 60;
        w.travel_to(c, Some(work), Purpose::Work);
        assert_eq!(w.citizens[c].mode, TravelMode::Walk);
        for _ in 0..120 {
            if w.citizens[c].inside == Some(work) {
                break;
            }
            w.tick(&mut minds);
        }
        assert_eq!(w.citizens[c].inside, Some(work), "arrived: {}", w.citizens[c].activity.label());
        assert!(w.clock.minute_of_day() < 9 * 60, "clocked in at {}", w.clock.format_hhmm());
    }

    #[test]
    fn a_station_is_road_connected_only_through_a_zoned_island() {
        use crate::rail::RailFacility;
        let mut w = World::new(WorldConfig::default());
        w.economy.treasury = 500_000.0;
        // Island A: the town. Island B: a street with homes, not joined to
        // A. Island C: a street in a field.
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        w.add_road(&[Vec2::new(200.0, 600.0), Vec2::new(1000.0, 600.0)], 1, 1);
        w.add_zone(square(300.0, 610.0, 150.0), LandUse::Residential);
        w.add_road(&[Vec2::new(-900.0, 700.0), Vec2::new(-500.0, 700.0)], 1, 1);
        let on_b = w.place_rail_facility(RailFacility::Station, Vec2::new(700.0, 560.0)).expect("station on island B");
        let on_c = w.place_rail_facility(RailFacility::Station, Vec2::new(-700.0, 660.0)).expect("station on island C");
        let town_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
        let b_lot = w.buildings[on_b].lot;
        assert!(w.roads.path(w.lots[b_lot].access_node, w.lots[town_lot].access_node).is_none(), "islands A and B are separate");
        assert!(w.buildings[on_b].rail.as_ref().unwrap().road_connected, "a street with homes on it is a city, whichever lot the map lists first");
        assert!(!w.buildings[on_c].rail.as_ref().unwrap().road_connected, "a street in a field is not");
        // Joining C to A connects it.
        w.add_road(&[Vec2::new(-500.0, 700.0), Vec2::new(-400.0, 0.0)], 1, 1);
        assert!(w.buildings[on_c].rail.as_ref().unwrap().road_connected);
    }

    #[test]
    fn a_save_with_dangling_ids_and_missing_fields_loads_cleanly() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
        let mut minds = IdleMind;
        for _ in 0..(10 * 60) {
            w.tick(&mut minds);
        }
        finish_all(&mut w);
        for _ in 0..120 {
            w.tick(&mut minds);
        }
        let c = w.citizens.keys().next().expect("someone moved in");
        // An id nothing points at any more.
        let ghost = {
            let any = w.buildings.values().next().unwrap().clone();
            let id = w.buildings.insert(any);
            w.buildings.remove(id);
            id
        };
        {
            let cz = &mut w.citizens[c];
            cz.home = Some(ghost);
            cz.work = Some(ghost);
            cz.inside = Some(ghost);
            cz.parked_at = Some(ghost);
            cz.activity = Activity::Visiting(ghost);
            cz.transit = Some(TransitTrip { leg: TransitLeg::Rail { board: ghost, alight: ghost }, target: ghost, purpose: Purpose::Home, since: 0 });
        }
        let lot = w.lots.keys().next().unwrap();
        w.lots[lot].building = Some(ghost);
        let text = w.to_ron().unwrap();
        assert!(text.contains("version: 1"));
        // An older save: no version, and a config field that did not exist yet.
        let text = text.replacen("version: 1,", "", 1);
        let text = {
            let start = text.find("bike_threshold:").unwrap();
            let end = start + text[start..].find('\n').unwrap() + 1;
            format!("{}{}", &text[..start], &text[end..])
        };
        let mut back = World::from_ron(&text).expect("loads");
        assert_eq!(back.version, SAVE_VERSION);
        assert_eq!(back.config.bike_threshold, WorldConfig::default().bike_threshold, "a missing field takes its default");
        let cz = &back.citizens[c];
        assert!(cz.home.is_none() && cz.work.is_none() && cz.inside.is_none() && cz.parked_at.is_none() && cz.transit.is_none());
        assert!(matches!(cz.activity, Activity::Idle));
        assert!(back.lots[lot].building.is_none());
        for _ in 0..200 {
            back.tick(&mut minds);
        }
    }

    #[test]
    fn the_overdraft_floor_refuses_purchases_and_doors_and_the_ledger_balances() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Commercial);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Entertainment);
        w.add_zone(square(-400.0, 10.0, 190.0), LandUse::Residential);
        let shop_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Commercial);
        let venue_lot = lot_near(&w, Vec2::new(-100.0, 20.0), LandUse::Entertainment);
        let home_lot = lot_near(&w, Vec2::new(-300.0, 20.0), LandUse::Residential);
        w.build_on(shop_lot);
        w.build_on(venue_lot);
        w.build_on(home_lot);
        let shop = w.lots[shop_lot].building.unwrap();
        let venue = w.lots[venue_lot].building.unwrap();
        finish_all(&mut w);
        w.buildings[shop].business = Some(Business::new(BusinessKind::GadgetShop, 4));
        w.buildings[shop].business.as_mut().unwrap().add(Resource::Goods, 5.0);
        assert_eq!(w.buildings[venue].business.as_ref().unwrap().kind, BusinessKind::Venue);
        let mut minds = IdleMind;
        for _ in 0..600 {
            if !w.citizens.is_empty() {
                break;
            }
            w.tick(&mut minds);
        }
        let c = w.citizens.keys().next().expect("someone moved in");
        // Skint: the shop sells nothing and the door is shut.
        w.citizens[c].savings = OVERDRAFT + 1.0;
        let mood0 = w.citizens[c].mood;
        w.buy(c, shop, Resource::Goods);
        assert_eq!(w.buildings[shop].business.as_ref().unwrap().stock_of(Resource::Goods), 5.0, "nothing sold");
        assert!(w.citizens[c].mood < mood0 && w.citizens[c].thought.contains("Skint"));
        let entrance = w.building_entrance(venue).unwrap();
        w.citizens[c].activity = Activity::Traveling { to: Some(venue), purpose: Purpose::Visit(LandUse::Entertainment), path: vec![entrance], idx: 1 };
        w.arrive(c);
        assert!(w.citizens[c].inside.is_none() && matches!(w.citizens[c].activity, Activity::Idle), "turned away");
        assert!(w.citizens[c].savings >= OVERDRAFT);
        // Flush: served and charged.
        w.citizens[c].savings = 100.0;
        w.citizens[c].activity = Activity::Traveling { to: Some(venue), purpose: Purpose::Visit(LandUse::Entertainment), path: vec![entrance], idx: 1 };
        w.arrive(c);
        assert_eq!(w.citizens[c].inside, Some(venue));
        assert!((w.citizens[c].savings - (100.0 - VENUE_COVER)).abs() < 0.01);
        w.citizens[c].inside = None;
        w.citizens[c].activity = Activity::Idle;
        w.buy(c, shop, Resource::Goods);
        assert_eq!(w.buildings[shop].business.as_ref().unwrap().stock_of(Resource::Goods), 4.0);
        // A shut venue is not somewhere to go out to.
        w.buildings[venue].business.as_mut().unwrap().closed = true;
        w.apply_decision(c, Decision { intent: Intent::Visit(LandUse::Entertainment), thought: String::new(), memory: None });
        assert!(matches!(w.citizens[c].activity, Activity::Idle), "nobody heads for a closed venue");
        // The ledger: every treasury move is booked, so the day's flows sum
        // to the day's change.
        let before = w.economy.treasury;
        let today0 = w.economy.ledger.today;
        let ind = lot_near(&w, Vec2::new(-350.0, 20.0), LandUse::Residential);
        w.lots[ind].land_use = LandUse::Industrial;
        w.build_power(ind, PowerPlantKind::Solar).expect("plant");
        assert!(w.economy.ledger.today_of(Flow::Construction) < 0.0);
        let booked: f32 = (0..9).map(|i| w.economy.ledger.today[i] - today0[i]).sum();
        assert!((w.economy.treasury - before - booked).abs() < 0.01, "treasury moved {} but the books say {}", w.economy.treasury - before, booked);
    }

    #[test]
    fn same_seed_and_script_give_identical_saves() {
        fn run() -> String {
            let mut w = World::new(WorldConfig { seed: 11, ..WorldConfig::default() });
            for i in -1..=1 {
                let c = i as f32 * 200.0;
                w.add_road(&[Vec2::new(-500.0, c), Vec2::new(500.0, c)], 1, 1);
                w.add_road(&[Vec2::new(c, -500.0), Vec2::new(c, 500.0)], 1, 1);
            }
            w.add_zone(square(-190.0, -190.0, 180.0), LandUse::Residential);
            w.add_zone(square(10.0, 10.0, 180.0), LandUse::Commercial);
            w.add_zone(square(-190.0, 10.0, 180.0), LandUse::Industrial);
            w.add_zone(square(10.0, -190.0, 60.0), LandUse::Park);
            let mut minds = IdleMind;
            for _ in 0..(36 * 60) {
                w.tick(&mut minds);
            }
            finish_all(&mut w);
            for _ in 0..(6 * 60) {
                w.tick(&mut minds);
            }
            assert!(!w.citizens.is_empty() && !w.traffic.cars.is_empty(), "something happened");
            w.to_ron().unwrap()
        }
        let (a, b) = (run(), run());
        assert!(a == b, "two runs of the same script diverged");
    }

    /// A block with a bus line, a depot and a bus out on the loop.
    fn bus_city() -> (World, RouteId) {
        use crate::transit;
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(-400.0, 140.0), Vec2::new(400.0, 140.0)], 1, 1);
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(-300.0, 140.0)], 1, 1);
        w.add_road(&[Vec2::new(300.0, 0.0), Vec2::new(300.0, 140.0)], 1, 1);
        w.add_zone(vec![Vec2::new(-390.0, 10.0), Vec2::new(-310.0, 10.0), Vec2::new(-310.0, 130.0), Vec2::new(-390.0, 130.0)], LandUse::Residential);
        w.add_zone(vec![Vec2::new(310.0, 10.0), Vec2::new(390.0, 10.0), Vec2::new(390.0, 130.0), Vec2::new(310.0, 130.0)], LandUse::Commercial);
        w.add_zone(vec![Vec2::new(-100.0, -150.0), Vec2::new(100.0, -150.0), Vec2::new(100.0, -10.0), Vec2::new(-100.0, -10.0)], LandUse::Industrial);
        let route = w.add_bus_route();
        for p in [Vec2::new(-200.0, 4.0), Vec2::new(200.0, 4.0), Vec2::new(296.0, 70.0), Vec2::new(200.0, 136.0), Vec2::new(-200.0, 136.0), Vec2::new(-296.0, 70.0)] {
            w.add_bus_stop(route, p).expect("stop snaps to the street");
        }
        let depot_lot = w.lots.iter().find(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Industrial && l.area() >= transit::DEPOT_MIN_AREA).map(|(id, _)| id).expect("industrial lot");
        w.build_depot(depot_lot).expect("depot built");
        (w, route)
    }

    #[test]
    fn a_save_mid_journey_keeps_trucks_buses_and_riders() {
        let (mut w, route) = bus_city();
        let home_lot = lot_near(&w, Vec2::new(-350.0, 70.0), LandUse::Residential);
        let shop_lot = lot_near(&w, Vec2::new(350.0, 70.0), LandUse::Commercial);
        w.build_on(home_lot);
        w.build_on(shop_lot);
        finish_all(&mut w);
        let mut minds = IdleMind;
        // Run until a bus is out and people have moved in; sites order
        // trucks of their own as the city grows.
        for _ in 0..(12 * 60) {
            w.tick(&mut minds);
        }
        let bus = w.traffic.cars.iter().find(|(_, c)| c.bus.is_some()).map(|(id, _)| id).expect("a bus is out");
        let riders: Vec<CitizenId> = w.citizens.keys().take(3).collect();
        assert!(!riders.is_empty(), "someone moved in");
        for &r in &riders {
            w.traffic.cars[bus].bus.as_mut().unwrap().passengers.push(r);
            let c = &mut w.citizens[r];
            c.inside = None;
            c.activity = Activity::Riding { bus };
            c.transit = Some(TransitTrip { leg: TransitLeg::Bus { route, board: 0, alight: 3 }, target: w.lots[shop_lot].building.unwrap(), purpose: Purpose::Shop(Resource::Goods), since: 0 });
        }
        // A truck on the road too.
        let shop = w.lots[shop_lot].building.unwrap();
        w.import(shop, Resource::Goods, 3.0, 10.0);
        let trucks = w.traffic.cars.values().filter(|c| c.cargo.is_some()).count();
        let cars = w.traffic.cars.len();
        let riding = w.citizens.values().filter(|c| matches!(c.activity, Activity::Riding { .. })).count();
        assert!(trucks >= 1 && riding == riders.len(), "trucks {trucks} riding {riding}");
        let mut back = World::from_ron(&w.to_ron().unwrap()).unwrap();
        assert_eq!(back.traffic.cars.len(), cars, "every car reattached to its lane");
        assert_eq!(back.traffic.cars.values().filter(|c| c.cargo.is_some()).count(), trucks);
        assert_eq!(back.traffic.cars[bus].bus.as_ref().unwrap().passengers.len(), riders.len());
        assert_eq!(back.citizens.values().filter(|c| matches!(c.activity, Activity::Riding { .. })).count(), riding);
        for _ in 0..200 {
            back.tick(&mut minds);
        }
        assert!(back.citizens.values().all(|c| c.pos.is_finite()));
    }

    #[test]
    fn a_business_that_cannot_pay_closes_goes_derelict_and_is_demolished() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Commercial);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
        let shop_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Commercial);
        let home_lot = lot_near(&w, Vec2::new(-100.0, 20.0), LandUse::Residential);
        w.build_on(shop_lot);
        w.build_on(home_lot);
        let shop = w.lots[shop_lot].building.unwrap();
        finish_all(&mut w);
        w.buildings[shop].business = Some(Business::new(BusinessKind::Bakery, w.buildings[shop].jobs));
        let mut minds = IdleMind;
        for _ in 0..600 {
            if !w.citizens.is_empty() {
                break;
            }
            w.tick(&mut minds);
        }
        let worker = w.citizens.keys().next().expect("someone moved in");
        if w.citizens[worker].work != Some(shop) {
            if let Some(old) = w.citizens[worker].work {
                w.buildings[old].workers.retain(|&c| c != worker);
            }
            w.citizens[worker].work = None;
            w.assign_job(worker, shop);
        }
        w.buildings[shop].business.as_mut().unwrap().money = -1000.0;
        w.drain_events();
        for day in 0..5 {
            assert!(!w.buildings[shop].business.as_ref().unwrap().closed, "still open on day {day}");
            w.daily_businesses();
        }
        let b = &w.buildings[shop];
        assert!(b.business.as_ref().unwrap().closed, "five broke days and it shuts");
        assert!(b.is_derelict(), "a shut shop is a derelict shell: condition {}", b.condition);
        assert!(b.workers.is_empty());
        assert_eq!(w.citizens[worker].work, None);
        assert_eq!(w.citizens[worker].income, GIG_INCOME);
        assert!(w.citizens[worker].memories.iter().any(|m| m.text.contains("shut down")));
        assert!(w.drain_events().iter().any(|e| matches!(e, SimEvent::BusinessClosed(x) if *x == shop)));
        assert_eq!(w.economy.market.closed_total, 1);
        for _ in 0..7 {
            w.daily_businesses();
            assert!(w.buildings.contains_key(shop), "a week of grace");
        }
        w.daily_businesses();
        assert!(!w.buildings.contains_key(shop), "then the lot is cleared");
        assert!(w.lots[shop_lot].building.is_none() || !w.buildings.contains_key(w.lots[shop_lot].building.unwrap()));
    }

    #[test]
    fn four_hungry_days_and_a_citizen_leaves() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        let home_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
        w.build_on(home_lot);
        let home = w.lots[home_lot].building.unwrap();
        w.finish_construction(home);
        let mut minds = IdleMind;
        for _ in 0..600 {
            if !w.citizens.is_empty() {
                break;
            }
            w.tick(&mut minds);
        }
        let c = w.citizens.keys().next().expect("someone moved in");
        assert!(w.buildings[home].residents.contains(&c));
        w.citizens[c].satiety = 0.0;
        w.citizens[c].savings = 1000.0;
        for day in 0..3 {
            w.daily_citizen(c);
            assert!(w.citizens.contains_key(c), "still here after {} hungry days", day + 1);
            assert_eq!(w.citizens[c].hungry_days, day + 1);
        }
        w.daily_citizen(c);
        assert!(!w.citizens.contains_key(c), "gone on the fourth");
        assert!(!w.buildings[home].residents.contains(&c));
        assert_eq!(w.economy.emigrated_total, 1);
        assert!(w.drain_events().iter().any(|e| matches!(e, SimEvent::CitizenLeft(x) if *x == c)));
    }

    #[test]
    fn job_search_fills_open_jobs_with_the_fixed_skill_formula() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Residential);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Commercial);
        let home_lot = lot_near(&w, Vec2::new(100.0, 20.0), LandUse::Residential);
        w.build_on(home_lot);
        w.finish_construction(w.lots[home_lot].building.unwrap());
        let mut minds = IdleMind;
        for _ in 0..600 {
            if !w.citizens.is_empty() {
                break;
            }
            w.tick(&mut minds);
        }
        let c = w.citizens.keys().next().expect("someone moved in");
        if let Some(old) = w.citizens[c].work {
            w.buildings[old].workers.retain(|&x| x != c);
        }
        w.lay_off(c, None);
        assert_eq!((w.citizens[c].work, w.citizens[c].income), (None, GIG_INCOME));
        // No jobs anywhere: nothing happens.
        for b in w.buildings.values_mut() {
            b.jobs = 0;
        }
        w.job_search();
        assert_eq!(w.citizens[c].work, None);
        // One opening, and it is filled at the wage times the person's skill.
        let shop_lot = lot_near(&w, Vec2::new(-100.0, 20.0), LandUse::Commercial);
        w.build_on(shop_lot);
        let shop = w.lots[shop_lot].building.unwrap();
        w.finish_construction(shop);
        w.buildings[shop].jobs = 1;
        w.job_search();
        assert_eq!(w.citizens[c].work, Some(shop));
        assert!(w.buildings[shop].workers.contains(&c));
        let expected = w.buildings[shop].wage() * World::skill_of(w.citizens[c].seed);
        assert!((w.citizens[c].income - expected).abs() < 0.01);
        assert_eq!(w.buildings[shop].free_jobs(), 0);
    }

    #[test]
    fn gini_is_zero_when_equal_and_near_one_when_one_has_everything() {
        assert_eq!(Economy::gini(&mut vec![]), 0.0);
        assert_eq!(Economy::gini(&mut vec![50.0]), 0.0);
        assert!(Economy::gini(&mut vec![40.0, 40.0, 40.0, 40.0]).abs() < 1e-6);
        let g = Economy::gini(&mut vec![0.0, 0.0, 0.0, 100.0]);
        assert!((g - 0.75).abs() < 1e-6, "(n-1)/n for a single earner: {g}");
        let mut unsorted = vec![100.0, 0.0, 0.0, 0.0];
        assert!((Economy::gini(&mut unsorted) - 0.75).abs() < 1e-6, "order does not matter");
        assert!(Economy::gini(&mut vec![10.0, 20.0, 30.0, 40.0]) < g);
        assert_eq!(Economy::gini(&mut vec![0.0, 0.0]), 0.0, "no income, no inequality");
    }

    #[test]
    fn removing_a_bus_stop_shifts_riders_and_buses_and_cancels_trips_through_it() {
        let (mut w, route) = bus_city();
        let home_lot = lot_near(&w, Vec2::new(-350.0, 70.0), LandUse::Residential);
        w.build_on(home_lot);
        finish_all(&mut w);
        let mut minds = IdleMind;
        for _ in 0..600 {
            if !w.citizens.is_empty() {
                break;
            }
            w.tick(&mut minds);
        }
        let shop = w.buildings.keys().next().unwrap();
        let ids: Vec<CitizenId> = w.citizens.keys().collect();
        let (a, b) = (ids[0], *ids.get(1).unwrap_or(&ids[0]));
        let trip = |board, alight| Some(TransitTrip { leg: TransitLeg::Bus { route, board, alight }, target: shop, purpose: Purpose::Shop(Resource::Goods), since: 0 });
        w.citizens[a].transit = trip(1, 4);
        w.citizens[a].activity = Activity::Waiting { at: Vec2::ZERO };
        if b != a {
            w.citizens[b].transit = trip(2, 5);
            w.citizens[b].activity = Activity::Waiting { at: Vec2::ZERO };
        }
        // A bus between stops 1 and 3, heading for 3.
        let stops = w.transit.routes[route].stops.clone();
        let (lanes, s0, s1) = w.traffic.net.find_route(stops[1].segment, stops[1].pos, stops[3].segment, stops[3].pos).expect("route");
        let car = w.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, 0);
        let depot = w.buildings.iter().find(|(_, b)| b.depot).map(|(id, _)| id).unwrap();
        w.traffic.cars[car].bus = Some(Bus { route, next_stop: 3, passengers: Vec::new(), to_depot: false, depot });
        w.remove_bus_stop(route, 2);
        assert_eq!(w.transit.routes[route].stops.len(), 5);
        assert_eq!(w.traffic.cars[car].bus.as_ref().unwrap().next_stop, 2, "the bus's next stop moved down one");
        assert_eq!(w.citizens[a].transit.map(|t| t.leg), Some(TransitLeg::Bus { route, board: 1, alight: 3 }), "indices past the gap shift");
        if b != a {
            assert!(w.citizens[b].transit.is_none() && matches!(w.citizens[b].activity, Activity::Idle), "a trip through the removed stop is cancelled");
        }
        // Removing a stop before the bus's target does not touch a bus heading home.
        w.traffic.cars[car].bus.as_mut().unwrap().next_stop = usize::MAX;
        w.remove_bus_stop(route, 0);
        assert_eq!(w.traffic.cars[car].bus.as_ref().unwrap().next_stop, usize::MAX);
        assert_eq!(w.citizens[a].transit.map(|t| t.leg), Some(TransitLeg::Bus { route, board: 0, alight: 2 }));
    }

    #[test]
    fn brownouts_cut_production() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 190.0), LandUse::Industrial);
        w.add_zone(square(-200.0, 10.0, 190.0), LandUse::Residential);
        let ind: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial).map(|(id, _)| id).collect();
        let res: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Residential).map(|(id, _)| id).collect();
        for &l in ind.iter().chain(res.iter()) {
            w.build_on(l);
        }
        finish_all(&mut w);
        // A fully staffed grain farm (no inputs, so output depends on power alone).
        let farm = w.lots[ind[0]].building.unwrap();
        {
            let b = &mut w.buildings[farm];
            b.tier = Tier::Low;
            b.jobs = 8;
            b.business = Some(Business::new(BusinessKind::GrainFarm, 8));
            b.workers = vec![CitizenId::default(); 8];
        }
        let full = w.buildings[farm].business.clone().unwrap().produce(8, 1.0);
        assert!(full > 0.0);
        // Lights on: an hour makes the full amount.
        let stock0 = w.buildings[farm].business.as_ref().unwrap().stock_of(Resource::Grain);
        w.hourly_economy();
        assert!(w.buildings[farm].powered >= 0.999, "powered {}", w.buildings[farm].powered);
        let made_lit = w.buildings[farm].business.as_ref().unwrap().stock_of(Resource::Grain) - stock0;
        assert!((made_lit - full).abs() < 1e-3, "lit: {made_lit} vs {full}");
        // Towers everywhere: demand outruns the grid and the cheap farm is
        // served last, so it makes less.
        for b in w.buildings.values_mut() {
            if b.lot != ind[0] {
                b.floors = 30;
                b.tier = Tier::High;
            }
        }
        let stock1 = w.buildings[farm].business.as_ref().unwrap().stock_of(Resource::Grain);
        w.hourly_economy();
        let p = w.buildings[farm].powered;
        assert!(p < 0.9, "the farm is browned out: {p}");
        assert!(w.economy.power.brownout_hours_today >= 1);
        let made_dark = w.buildings[farm].business.as_ref().unwrap().stock_of(Resource::Grain) - stock1;
        assert!(made_dark < made_lit * 0.9, "dark: {made_dark} vs lit {made_lit}");
        assert!((made_dark - full * p).abs() < 1e-3, "output scales with power: {made_dark} vs {}", full * p);
    }
}
