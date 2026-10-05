use crate::buildings::{Building, BuildingId};
use crate::citizens::{Activity, Citizen, CitizenId, Purpose};
use crate::clock::SimClock;
use crate::economy::Economy;
use crate::events::SimEvent;
use crate::mind::{Decision, Intent, MindContext, MindProvider};
use crate::names;
use crate::roads::{NodeId, RoadNetwork};
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
    /// Metres per tick (minute).
    pub walk_speed: f32,
    pub drive_speed: f32,
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
            walk_speed: 80.0,
            drive_speed: 450.0,
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
    pub roads: RoadNetwork,
    pub zones: SlotMap<ZoneId, Zone>,
    pub lots: SlotMap<LotId, Lot>,
    pub buildings: SlotMap<BuildingId, Building>,
    pub citizens: SlotMap<CitizenId, Citizen>,
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
            roads: RoadNetwork::default(),
            zones: SlotMap::with_key(),
            lots: SlotMap::with_key(),
            buildings: SlotMap::with_key(),
            citizens: SlotMap::with_key(),
            economy: Economy::default(),
            events: Vec::new(),
            next_seed: 1,
        }
    }

    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default().depth_limit(6))
    }

    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(text)
    }

    fn fresh_seed(&mut self) -> u64 {
        self.next_seed = self.next_seed.wrapping_add(1);
        self.next_seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ self.config.seed
    }

    pub fn drain_events(&mut self) -> Vec<SimEvent> {
        std::mem::take(&mut self.events)
    }

    // ── player actions ───────────────────────────────────────────────────────

    pub fn add_road(&mut self, points: &[Vec2], lanes: u8) {
        let created = self.roads.add_polyline(points, lanes);
        if created.is_empty() {
            return;
        }
        self.events.push(SimEvent::RoadsChanged);
        self.regenerate_lots();
    }

    pub fn add_zone(&mut self, polygon: Vec<Vec2>, land_use: LandUse) -> Option<ZoneId> {
        if polygon.len() < 3 {
            return None;
        }
        let id = self.zones.insert(Zone { polygon, land_use });
        self.regenerate_lots();
        Some(id)
    }

    fn regenerate_lots(&mut self) {
        let mut any = false;
        let ids: Vec<ZoneId> = self.zones.keys().collect();
        for zid in ids {
            let zone = self.zones[zid].clone();
            let created = generate_lots(zid, &zone, &self.roads, &mut self.lots);
            any |= !created.is_empty();
        }
        if any {
            self.events.push(SimEvent::LotsChanged);
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
        self.step_citizens(minds);
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
        let vacant: Vec<LotId> = self
            .lots
            .iter()
            .filter(|(_, l)| l.building.is_none())
            .map(|(id, _)| id)
            .collect();
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

    fn build_on(&mut self, lot_id: LotId) {
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
        let far: Vec<NodeId> = {
            let max = leaves.iter().map(|&n| self.roads.node(n).pos.length()).fold(0.0, f32::max);
            leaves.iter().copied().filter(|&n| self.roads.node(n).pos.length() >= max * 0.6).collect()
        };
        far.choose(&mut self.rng).copied()
    }

    fn pick_building<F: Fn(&Building) -> bool>(&mut self, pred: F) -> Option<BuildingId> {
        let candidates: Vec<BuildingId> =
            self.buildings.iter().filter(|(_, b)| pred(b)).map(|(id, _)| id).collect();
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
            citizen.remember(
                tick,
                format!("Rolled into Neon District on day {} at {}.", self.clock.day() + 1, self.clock.format_hhmm()),
            );
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
        let jobless: Vec<CitizenId> =
            self.citizens.iter().filter(|(_, c)| c.work.is_none()).map(|(id, _)| id).collect();
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
            // Movement.
            let arrived = {
                let c = &mut self.citizens[id];
                if let Activity::Traveling { path, idx, purpose, .. } = &mut c.activity {
                    let speed = match purpose {
                        Purpose::Home | Purpose::Work => self.config.drive_speed,
                        _ => self.config.walk_speed,
                    };
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
            Activity::Traveling { to, purpose, .. } => (*to, *purpose),
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
                let target = self.pick_building(|b| b.land_use == lu && b.floors > 0 || b.land_use == lu && lu == LandUse::Park);
                if let Some(b) = target {
                    if inside != Some(b) {
                        self.travel_to(id, Some(b), Purpose::Visit(lu));
                    }
                }
            }
            Intent::Wander => {
                self.wander(id);
            }
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

    fn start_node_for(&self, id: CitizenId) -> Option<NodeId> {
        let c = &self.citizens[id];
        if let Some(b) = c.inside {
            return self.building_lot(b).map(|l| l.access_node);
        }
        self.roads.nearest_on_network(c.pos).map(|(sid, t, _)| self.roads.nearest_endpoint(sid, t))
    }

    fn travel_to(&mut self, id: CitizenId, target: Option<BuildingId>, purpose: Purpose) {
        let Some(target) = target else { return };
        let Some(start) = self.start_node_for(id) else {
            self.citizens[id].activity = Activity::Idle;
            return;
        };
        let Some(lot) = self.building_lot(target) else { return };
        let end = lot.access_node;
        let entrance = lot.corners[0].lerp(lot.corners[1], 0.5);
        let Some(nodes) = self.roads.path(start, end) else {
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
        let Some(start) = self.start_node_for(id) else { return };
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

    #[test]
    fn city_grows_from_roads_and_zones() {
        let mut w = World::new(WorldConfig::default());
        w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 2);
        w.add_road(&[Vec2::new(0.0, -400.0), Vec2::new(0.0, 400.0)], 2);
        w.add_zone(
            vec![Vec2::new(10.0, 10.0), Vec2::new(200.0, 10.0), Vec2::new(200.0, 200.0), Vec2::new(10.0, 200.0)],
            LandUse::Residential,
        );
        w.add_zone(
            vec![Vec2::new(-200.0, 10.0), Vec2::new(-10.0, 10.0), Vec2::new(-10.0, 200.0), Vec2::new(-200.0, 200.0)],
            LandUse::Commercial,
        );
        assert!(!w.lots.is_empty(), "lots should be carved along the roads");
        let mut minds = IdleMind;
        for _ in 0..(24 * 60) {
            w.tick(&mut minds);
        }
        assert!(!w.buildings.is_empty(), "buildings should develop within a day");
        assert!(!w.citizens.is_empty(), "citizens should immigrate once homes exist");
        let text = w.to_ron().unwrap();
        let back = World::from_ron(&text).unwrap();
        assert_eq!(back.citizens.len(), w.citizens.len());
    }
}
