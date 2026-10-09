//! Who comes to town and where they work: immigration, jobs, taxes by home.

use super::*;

impl World {
    pub(in crate::world) fn entry_node(&mut self) -> Option<NodeId> {
        let leaves = self.city_leaves();
        if leaves.is_empty() {
            return None;
        }
        let max = leaves.iter().map(|&n| self.roads.node(n).pos.length()).fold(0.0, f32::max);
        let far: Vec<NodeId> = leaves.iter().copied().filter(|&n| self.roads.node(n).pos.length() >= max * 0.6).collect();
        far.choose(&mut self.rng).copied()
    }

    pub(in crate::world) fn pick_building<F: Fn(&Building) -> bool>(&mut self, pred: F) -> Option<BuildingId> {
        let candidates: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| pred(b)).map(|(id, _)| id).collect();
        candidates.choose(&mut self.rng).copied()
    }

    /// An open job for someone arriving or looking: the least-staffed
    /// employer first, food sellers (grocers, bakeries) well ahead of the
    /// rest and the farms, vats and mills that feed them next, so a growing
    /// town staffs its food chain before its sites and plants and nobody
    /// goes hungry beside full warehouses. `taken` are jobs already promised
    /// to others in the same household. Ties are broken at random so the
    /// same firm is not always picked.
    pub(in crate::world) fn pick_job_except(&mut self, taken: &[BuildingId]) -> Option<BuildingId> {
        // Upstream food businesses (farms, vats, mills) jump the queue only
        // while their output is short (under a day of it in stock): a farm
        // sitting on unsold grain needs no more hands.
        let bonus = |b: &Building| match b.business.as_ref() {
            Some(z) if z.closed => 0.0,
            Some(z) if z.sells(Resource::Groceries) || z.sells(Resource::BakedGoods) => 0.5,
            Some(z) if z.kind.is_food() => {
                let day = z.hourly_capacity(b.jobs, 1.0) * 24.0;
                if z.kind.recipe().outputs.iter().any(|(r, per)| z.stock_of(*r) < per * day) { 0.3 } else { 0.0 }
            }
            _ => 0.0,
        };
        let scored: Vec<(f32, BuildingId)> = self
            .buildings
            .iter()
            .filter(|(id, b)| b.free_jobs() as usize > taken.iter().filter(|t| *t == id).count())
            // [under-construction] Open, or a site hiring its crew: a
            // station or plant whose lot is still being cleared hires
            // nobody yet.
            .filter(|(_, b)| b.operating() || b.construction.is_some())
            // [dock-access] A dock or station off the roads hires nobody yet.
            // [sweep-sim] One people can walk to (a ferry terminal or a
            // station joined by a path) hires: its staff walk in too.
            .filter(|(id, b)| !((b.port.is_some() || b.rail.is_some()) && self.access_serves(*id) == Some(false)))
            .map(|(id, b)| (b.staffing() - bonus(b), id))
            .collect();
        let best = scored.iter().map(|(s, _)| *s).fold(f32::INFINITY, f32::min);
        let near_best: Vec<BuildingId> = scored.iter().filter(|(s, _)| *s <= best + 0.1).map(|(_, id)| *id).collect();
        near_best.choose(&mut self.rng).copied()
    }

    pub(in crate::world) fn pick_job(&mut self) -> Option<BuildingId> {
        self.pick_job_except(&[])
    }

    /// Households per arrival: one to three, plus more while homes and jobs
    /// stand open, so the town fills about as fast as it is built (a fixed
    /// trickle left a thousand homes empty and the shops unstaffed).
    /// While there is no work and a good share of the town's adults are
    /// already out of it, word gets round: households come only now and then.
    pub(in crate::world) fn arrivals(&mut self, free_homes: u32) -> u32 {
        let free_jobs: u32 = self.buildings.values().map(|b| b.free_jobs() as u32).sum();
        if free_jobs == 0 && self.economy.unemployment_share() > JOBLESS_SLOWS_ARRIVALS {
            return u32::from(self.rng.random_bool(0.3)).min(free_homes);
        }
        let pull = free_homes.min(free_jobs + 10) / 40;
        let n = (self.rng.random_range(1..=3) + pull).min(MAX_ARRIVALS).min(free_homes);
        // [tax] Word of the income tax gets round: the free homes' tiers,
        // weighted by how many each has, set how keen people are to come.
        let factor = self.residential_tax_pull();
        if factor == 1.0 {
            return n;
        }
        let want = n as f32 * factor;
        let whole = want.floor();
        let extra = u32::from(self.rng.random::<f32>() < want - whole);
        (whole as u32 + extra).min(MAX_ARRIVALS).min(free_homes)
    }

    /// [tax] The line a citizen's income is taxed under: the tier of their
    /// home (the street and the unhoused count as low).
    pub fn income_tax_category(&self, cid: CitizenId) -> crate::tax::TaxCategory {
        self.tax_category_of_home(self.citizens.get(cid).and_then(|c| c.home))
    }

    pub(in crate::world) fn tax_category_of_home(&self, home: Option<BuildingId>) -> crate::tax::TaxCategory {
        let tier = home.and_then(|h| self.buildings.get(h)).map_or(Tier::Low, |b| b.tier);
        crate::tax::TaxCategory::residential(tier)
    }

    /// [tax] How the income tax pulls on newcomers: the pull of each tier,
    /// weighted by its free homes (1 at the standard rates).
    pub(in crate::world) fn residential_tax_pull(&self) -> f32 {
        let t = &self.economy.taxes;
        if Tier::ALL.iter().all(|&tier| t.pull(crate::tax::TaxCategory::residential(tier)) == 1.0) {
            return 1.0;
        }
        let mut free = [0.0f32; 3];
        for b in self.buildings.values().filter(|b| b.land_use == LandUse::Residential && b.operating()) {
            free[b.tier.index()] += b.free_homes() as f32;
        }
        let total: f32 = free.iter().sum();
        if total <= 0.0 {
            return t.land_use_pull(LandUse::Residential);
        }
        Tier::ALL.iter().map(|&tier| free[tier.index()] * t.pull(crate::tax::TaxCategory::residential(tier))).sum::<f32>() / total
    }

    /// The ages of a household arriving: singles, couples, retired
    /// couples, single parents and families with up to three children,
    /// sometimes with a grandparent.
    pub(in crate::world) fn household_ages(&mut self) -> Vec<u8> {
        let rng = &mut self.rng;
        let roll: f32 = rng.random();
        let size = if roll < 0.34 {
            1
        } else if roll < 0.64 {
            2
        } else if roll < 0.80 {
            3
        } else if roll < 0.93 {
            4
        } else {
            5
        };
        let adult = |rng: &mut SimRng| rng.random_range(19..60u8);
        let retiree = |rng: &mut SimRng| rng.random_range(67..88u8);
        let child = |rng: &mut SimRng| rng.random_range(0..17u8);
        match size {
            1 => vec![if rng.random_bool(0.15) { retiree(rng) } else { adult(rng) }],
            2 => {
                let r: f32 = rng.random();
                if r < 0.15 {
                    vec![retiree(rng), retiree(rng)]
                } else if r < 0.25 {
                    vec![rng.random_range(24..50u8), child(rng)]
                } else {
                    vec![adult(rng), adult(rng)]
                }
            }
            n => {
                let mut ages = vec![rng.random_range(24..50u8), rng.random_range(24..50u8)];
                for _ in 2..n {
                    ages.push(child(rng));
                }
                if n >= 4 && rng.random_bool(0.2) {
                    ages[n - 1] = retiree(rng);
                }
                ages
            }
        }
    }

    pub(in crate::world) fn immigrate(&mut self) {
        let free_homes: u32 = self.buildings.values().map(|b| b.free_homes() as u32).sum();
        if free_homes == 0 {
            self.economy.waiting_immigrants = (self.economy.waiting_immigrants + 1).min(60);
            return;
        }
        // In from elsewhere: by a road off the map (the highway once a ramp
        // joins it) or by train at a working station. With neither, nobody
        // can come.
        let entry = self.entry_node();
        if entry.is_none() && self.stations.is_empty() {
            return;
        }
        let n = self.arrivals(free_homes);
        for _ in 0..n {
            // Income first: a job for each adult if one is open, gig money
            // otherwise, pensions for the retired. Then a home they can
            // afford, the best tier available.
            let ages = self.household_ages();
            let mut people: Vec<(u8, u64, Option<BuildingId>, f32)> = Vec::with_capacity(ages.len());
            let mut taken: Vec<BuildingId> = Vec::new();
            for age in ages {
                let seed = self.fresh_seed();
                let skill = Self::skill_of(seed);
                let (job, income) = match LifeStage::of_age(age) {
                    LifeStage::Child => (None, 0.0),
                    LifeStage::Retiree => (None, PENSION),
                    LifeStage::Adult => {
                        let job = self.pick_job_except(&taken);
                        taken.extend(job);
                        (job, job.and_then(|j| self.buildings.get(j)).map(|b| b.wage() * skill).unwrap_or(GIG_INCOME * skill))
                    }
                };
                people.push((age, seed, job, income));
            }
            let income: f32 = people.iter().map(|p| p.3).sum();
            let family = people.iter().any(|p| LifeStage::of_age(p.0) == LifeStage::Child);
            let Some(home) = self.choose_home(income, None, family) else {
                self.economy.priced_out_total += 1;
                self.economy.waiting_immigrants = (self.economy.waiting_immigrants + 1).min(60);
                self.economy.housing_demand_by_tier[Self::tier_affordable(income).index()] += 1.0;
                break;
            };
            let tick = self.clock.tick;
            let by_train = if !self.stations.is_empty() && (entry.is_none() || self.rng.random_bool(0.6)) { self.stations.choose(&mut self.rng).copied() } else { None };
            // Off the train at the station; otherwise by road: drivers bring
            // their car in from the edge (`arrive_by_car`), the rest are
            // dropped at the door (the road end is out of sight, miles off).
            let pos = by_train
                .and_then(|s| self.building_entrance(s))
                .or_else(|| self.building_entrance(home))
                .or_else(|| self.building_lot(home).map(|l| l.center))
                .unwrap_or_default();
            let earners = people.iter().filter(|p| p.3 > 0.0).count().max(1) as f32;
            let (car_chance, bike_chance) = match Tier::for_income(income / earners) {
                Tier::Low => (0.12, 0.6),
                Tier::Mid => (0.6, 0.35),
                Tier::High => (0.92, 0.35),
            };
            // A few days of groceries in the boot, and some money.
            let size = people.len() as f32;
            let money = self.rng.random_range(40.0..160.0) * size.sqrt();
            let hid = self.households.insert(Household {
                members: Vec::new(),
                home: Some(home),
                money,
                groceries: size * GROCERIES_PER_PERSON_DAY * 3.0,
                rent_stress_days: 0,
                moved_day: self.clock.day(),
            });
            self.buildings[home].households.push(hid);
            let home_name = self.buildings[home].name.clone();
            let family = names::family_name(&mut self.rng);
            // [looks] Drawn from the seeds alone, so the world's dice roll
            // exactly as before.
            let class = Tier::for_income(income / earners);
            let looks = crate::looks::family_looks(&people.iter().map(|p| (p.1, p.0)).collect::<Vec<_>>(), class);
            for ((age, seed, job, income), look) in people.into_iter().zip(looks) {
                let stage = LifeStage::of_age(age);
                let name = names::member_name(&mut self.rng, &family);
                let owns_car = stage != LifeStage::Child && self.rng.random_bool(car_chance);
                let vehicle = owns_car.then(|| VehicleModel::for_owner(income, age, crate::vehicle::hedonism_of(seed), &mut crate::vehicle::model_rng(seed, 1)));
                let owns_bike = self.rng.random_bool(bike_chance);
                let savings = if stage == LifeStage::Child { 0.0 } else { self.rng.random_range(10.0..40.0) };
                let next_decision_tick = tick + self.rng.random_range(5..30);
                let mut citizen = Citizen {
                    mode: TravelMode::Walk,
                    owns_car,
                    owns_bike,
                    vehicle,
                    thought: "New city. New me.".to_string(),
                    next_decision_tick,
                    mood: 0.6,
                    income,
                    savings,
                    look: Some(look),
                    ..Citizen::newcomer(name, seed, age, Some(home), Some(hid), pos, tick)
                };
                citizen.remember(tick, format!("Rolled into Neon District on day {} at {}.", self.clock.day() + 1, self.clock.format_hhmm()));
                if let Some(s) = by_train {
                    citizen.remember(tick, format!("Stepped off the train at {}.", self.buildings[s].name));
                }
                citizen.remember(tick, format!("Moved into {home_name}."));
                let id = self.citizens.insert(citizen);
                self.households[hid].members.push(id);
                self.buildings[home].residents.push(id);
                if let Some(job) = job {
                    if self.buildings.get(job).is_some_and(|b| b.free_jobs() > 0) {
                        self.assign_job(id, job);
                    }
                }
                // [carlife] Car owners drive in from out of town (a highway
                // or the road at the map edge) with the car; without a road
                // in, they come without it.
                let drove = match vehicle {
                    Some(model) => self.arrive_by_car(id, model, home),
                    None => false,
                };
                if !drove {
                    let c = &mut self.citizens[id];
                    c.owns_car = false;
                    c.vehicle = None;
                    self.travel_to(id, Some(home), Purpose::Home);
                }
                self.events.push(SimEvent::CitizenArrived(id));
            }
            let groceries = self.households[hid].groceries;
            for &m in &self.households[hid].members {
                self.citizens[m].groceries = groceries;
            }
            self.economy.waiting_immigrants = self.economy.waiting_immigrants.saturating_sub(1);
        }
    }

    pub(in crate::world) fn assign_job(&mut self, id: CitizenId, job: BuildingId) {
        self.citizens[id].work = Some(job);
        self.citizens[id].income = Self::base_income(&self.buildings[job], self.citizens[id].seed);
        self.buildings[job].workers.push(id);
        let name = self.buildings[job].name.clone();
        let tick = self.clock.tick;
        self.citizens[id].remember(tick, format!("Got hired at {name}."));
    }

    /// Adults without a job take an open one, food chain first.
    pub(in crate::world) fn job_search(&mut self) {
        let jobless: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.work.is_none() && c.can_work()).map(|(id, _)| id).collect();
        for id in jobless {
            let Some(job) = self.pick_job() else { break };
            self.assign_job(id, job);
        }
    }
}
