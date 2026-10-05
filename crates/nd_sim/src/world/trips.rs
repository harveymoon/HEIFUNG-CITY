//! Citizens: daily upkeep, immigration and emigration, jobs, decisions,
//! journeys (`travel_to`, `arrive`, `wander`) and footfall.

use super::*;

/// Most newcomers in one arrival, however empty the town.
const MAX_ARRIVALS: u32 = 12;

impl World {
    pub(super) fn daily_citizen(&mut self, cid: CitizenId) {
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

    pub(super) fn move_home(&mut self, cid: CitizenId, new_home: BuildingId, template: &str) {
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

    pub(super) fn emigrate(&mut self, cid: CitizenId) {
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

    pub(super) fn entry_node(&mut self) -> Option<NodeId> {
        let leaves = self.roads.leaf_nodes();
        if leaves.is_empty() {
            return None;
        }
        let max = leaves.iter().map(|&n| self.roads.node(n).pos.length()).fold(0.0, f32::max);
        let far: Vec<NodeId> = leaves.iter().copied().filter(|&n| self.roads.node(n).pos.length() >= max * 0.6).collect();
        far.choose(&mut self.rng).copied()
    }

    pub(super) fn pick_building<F: Fn(&Building) -> bool>(&mut self, pred: F) -> Option<BuildingId> {
        let candidates: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| pred(b)).map(|(id, _)| id).collect();
        candidates.choose(&mut self.rng).copied()
    }

    /// An open job for someone arriving or looking: the least-staffed
    /// employer first, and food sellers (grocers, bakeries) ahead of the
    /// rest, so a growing town staffs its shops before its sites and plants
    /// and nobody goes hungry beside full warehouses. Ties are broken at
    /// random so the same firm is not always picked.
    pub(super) fn pick_job(&mut self) -> Option<BuildingId> {
        let food = |b: &Building| b.business.as_ref().is_some_and(|z| z.sells(Resource::Groceries) || z.sells(Resource::BakedGoods));
        let scored: Vec<(f32, BuildingId)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.free_jobs() > 0)
            .map(|(id, b)| (b.staffing() - if food(b) { 0.5 } else { 0.0 }, id))
            .collect();
        let best = scored.iter().map(|(s, _)| *s).fold(f32::INFINITY, f32::min);
        let near_best: Vec<BuildingId> = scored.iter().filter(|(s, _)| *s <= best + 0.1).map(|(_, id)| *id).collect();
        near_best.choose(&mut self.rng).copied()
    }

    /// Newcomers per arrival: one to three, plus more while homes and jobs
    /// stand open, so the town fills about as fast as it is built (a fixed
    /// trickle left a thousand homes empty and the shops unstaffed).
    pub(super) fn arrivals(&mut self, free_homes: u32) -> u32 {
        let free_jobs: u32 = self.buildings.values().map(|b| b.free_jobs() as u32).sum();
        let pull = free_homes.min(free_jobs + 10) / 40;
        (self.rng.random_range(1..=3) + pull).min(MAX_ARRIVALS).min(free_homes)
    }

    pub(super) fn immigrate(&mut self) {
        let free_homes: u32 = self.buildings.values().map(|b| b.free_homes() as u32).sum();
        if free_homes == 0 {
            self.economy.waiting_immigrants = (self.economy.waiting_immigrants + 1).min(60);
            return;
        }
        let Some(entry) = self.entry_node() else { return };
        let n = self.arrivals(free_homes);
        for _ in 0..n {
            // Income first: a job if one is open, gig money otherwise. Then a
            // home they can afford, the best tier available.
            let job = self.pick_job();
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

    pub(super) fn assign_job(&mut self, id: CitizenId, job: BuildingId) {
        self.citizens[id].work = Some(job);
        let skill = Self::skill_of(self.citizens[id].seed);
        self.citizens[id].income = self.buildings[job].wage() * skill;
        self.buildings[job].workers.push(id);
        let name = self.buildings[job].name.clone();
        let tick = self.clock.tick;
        self.citizens[id].remember(tick, format!("Got hired at {name}."));
    }

    pub(super) fn job_search(&mut self) {
        let jobless: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.work.is_none()).map(|(id, _)| id).collect();
        for id in jobless {
            let Some(job) = self.pick_job() else { break };
            self.assign_job(id, job);
        }
    }

    pub(super) fn step_citizens(&mut self, minds: &mut dyn MindProvider) {
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

    pub(super) fn arrive(&mut self, id: CitizenId) {
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

    pub(super) fn apply_decision(&mut self, id: CitizenId, d: Decision) {
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

    pub(super) fn leave_building(&mut self, id: CitizenId) -> Option<Vec2> {
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
    pub(super) fn origin_for(&self, id: CitizenId) -> Option<(SegmentId, Vec2, NodeId)> {
        let c = &self.citizens[id];
        if let Some(b) = c.inside {
            let lot = self.building_lot(b)?;
            let entrance = lot.entrance();
            return Some((lot.front_segment, entrance, lot.access_node));
        }
        let (sid, t, q) = self.roads.nearest_on_network(c.pos)?;
        Some((sid, q, self.roads.nearest_endpoint(sid, t)))
    }

    pub(super) fn travel_to(&mut self, id: CitizenId, target: Option<BuildingId>, purpose: Purpose) {
        self.travel_to_impl(id, target, purpose, true, true);
    }

    /// Start a journey: on foot, by bike, by car, or by bus or train with a
    /// walk at each end. `fresh` is false for the legs after a ride, so a
    /// journey's footfall is noted once, for the leg that began it.
    pub(super) fn travel_to_impl(&mut self, id: CitizenId, target: Option<BuildingId>, purpose: Purpose, allow_bus: bool, fresh: bool) {
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
    pub(super) fn note_footfall(&mut self, id: CitizenId, walk_nodes: &[u32]) {
        let lots = self.walk.lots_passed(walk_nodes);
        if let Some(c) = self.citizens.get_mut(id) {
            c.pending_footfall = lots;
        }
    }

    /// Everyone on foot who passes a lot is a potential customer: credit
    /// the lots of the journey just completed, once.
    pub(super) fn settle_footfall(&mut self, id: CitizenId) {
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
    pub(super) fn route_path(&self, nodes: &[NodeId], mode: TravelMode) -> Vec<Vec2> {
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

    pub(super) fn wander(&mut self, id: CitizenId) {
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
pub(super) const VENUE_COVER: f32 = 15.0;
