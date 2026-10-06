//! Citizens: daily upkeep, immigration and emigration, jobs, decisions,
//! journeys (`travel_to`, `arrive`, `wander`) and footfall.

use super::*;
use crate::citizens::{Household, HouseholdId, LifeStage, GROCERIES_PER_PERSON_DAY, PENSION, POCKET_MONEY};
use crate::vehicle::{VehicleClass, VehicleModel};

/// Most households in one arrival, however empty the town.
const MAX_ARRIVALS: u32 = 12;
/// Share of working-age adults out of work above which, with no job free,
/// households mostly stop arriving.
const JOBLESS_SLOWS_ARRIVALS: f32 = 0.05;
/// Households look for homes whose rent is at most this share of income.
pub const AFFORDABLE_SHARE: f32 = 0.35;
/// Rent over this share of income is stress: they move somewhere cheaper,
/// or leave once the money is gone.
pub const RENT_STRESS_SHARE: f32 = 0.5;

impl World {
    pub(super) fn daily_citizens(&mut self) {
        let cids: Vec<CitizenId> = self.citizens.keys().collect();
        for cid in cids {
            self.daily_citizen(cid);
        }
    }

    /// Once a day, per citizen: tax on the day's income, hunger days and
    /// what they do to mood. Four hungry days and they leave. Children left
    /// without a grown-up keep the home until the rent runs them out of
    /// money (`daily_household`), which is soon.
    pub(super) fn daily_citizen(&mut self, cid: CitizenId) {
        let tick = self.clock.tick;
        let Some(c) = self.citizens.get_mut(cid) else { return };
        self.economy.tax_today += c.income * 0.12;
        self.economy.credit(Flow::IncomeTax, c.income * 0.12);
        if c.satiety < 0.25 {
            c.hungry_days = c.hungry_days.saturating_add(1);
            c.mood = (c.mood - 0.1).max(0.0);
        } else {
            c.hungry_days = 0;
        }
        if c.hungry_days < 4 {
            return;
        }
        c.remember(tick, "Couldn't keep myself fed here. Leaving.".to_string());
        self.emigrate(cid);
    }

    /// Everyone's daily earnings: wages, gig money or a pension.
    pub fn household_income(&self, hid: HouseholdId) -> f32 {
        self.households.get(hid).map_or(0.0, |h| h.members.iter().filter_map(|m| self.citizens.get(*m)).map(|c| c.income).sum())
    }

    /// Rent of the household's home over its income (infinite with no
    /// income), or `None` when it has no home.
    pub fn household_rent_share(&self, hid: HouseholdId) -> Option<f32> {
        let rent = self.households.get(hid)?.home.and_then(|b| self.buildings.get(b))?.rent;
        let income = self.household_income(hid);
        Some(if income > 0.0 { rent / income } else { f32::INFINITY })
    }

    /// The rent share of the household a citizen belongs to.
    pub fn rent_share_of(&self, cid: CitizenId) -> Option<f32> {
        self.household_rent_share(self.citizens.get(cid)?.household?)
    }

    /// Once a day, per household: earnings in, rent and living costs out,
    /// pocket money for the grown-ups, wheels, rent stress and moving.
    pub(super) fn daily_household(&mut self, hid: HouseholdId) {
        let tick = self.clock.tick;
        let today = self.clock.day();
        let Some(hh) = self.households.get(hid) else { return };
        let members: Vec<CitizenId> = hh.members.iter().copied().filter(|m| self.citizens.contains_key(*m)).collect();
        if members.is_empty() {
            self.remove_household(hid);
            return;
        }
        let home = hh.home.filter(|h| self.buildings.contains_key(*h));
        let rent = home.map(|h| self.buildings[h].rent).unwrap_or(0.0);
        let income = self.household_income(hid);
        let living: f32 = members.iter().map(|m| if self.citizens[*m].stage() == LifeStage::Child { 10.0 } else { 20.0 }).sum();
        // Pockets come back to the shared purse (with any debts run up on
        // them), then the grown-ups draw pocket money again if there is some.
        let mut money = self.households[hid].money + income - rent - living;
        for &m in &members {
            money += std::mem::take(&mut self.citizens[m].savings);
        }
        let grown: Vec<CitizenId> = members.iter().copied().filter(|m| self.citizens[*m].stage() != LifeStage::Child).collect();
        if money > 0.0 {
            let pocket = (money * 0.5 / grown.len().max(1) as f32).min(POCKET_MONEY);
            for &m in &grown {
                self.citizens[m].savings = pocket;
                money -= pocket;
            }
        }
        // Wheels: a bike is cheap and first; a car needs a middle income and
        // money in the bank; the poor sell a car they cannot keep.
        for &m in &grown {
            let c = &mut self.citizens[m];
            if !c.owns_bike && money > 60.0 && self.rng.random_bool(0.08) {
                c.owns_bike = true;
                money -= 45.0;
                c.remember(tick, "Bought a bike. The city just got smaller.");
            }
            // What they buy follows income, age and taste (`VehicleModel::
            // for_owner`): the poor an old banger or a scooter, which they
            // can afford with less in the bank; the rich trade up an old one.
            let wants = |c: &crate::citizens::Citizen| VehicleModel::for_owner(c.income, c.age, crate::vehicle::hedonism_of(c.seed), &mut crate::vehicle::model_rng(c.seed, today as u64 + 2));
            if !c.owns_car && c.class() != Tier::Low && money > 400.0 && self.rng.random_bool(0.04) {
                let model = wants(c);
                c.owns_car = true;
                money -= model.price().min(300.0);
                c.vehicle = Some(model);
                c.remember(tick, format!("Bought a {}.", model_name(&model)));
            } else if !c.owns_car && c.class() == Tier::Low && money > 150.0 && self.rng.random_bool(0.02) {
                let model = wants(c);
                c.owns_car = true;
                money -= model.price().min(150.0);
                c.vehicle = Some(model);
                c.remember(tick, format!("Picked up a {}. It runs, mostly.", model_name(&model)));
            } else if c.owns_car && c.class() == Tier::High && money > 1500.0 && c.vehicle.is_some_and(|v| v.class == VehicleClass::Cheap) && self.rng.random_bool(0.05) {
                let model = wants(c);
                money -= model.price().min(800.0);
                c.vehicle = Some(model);
                c.remember(tick, format!("Traded the old wreck in for a {}.", model_name(&model)));
            }
            if c.owns_car && c.class() == Tier::Low && money < -100.0 {
                c.owns_car = false;
                money += c.vehicle.map_or(150.0, |v| (v.price() * 0.5).min(150.0));
                c.vehicle = None;
                c.remember(tick, "Sold the car. Couldn't afford to keep it.");
            }
        }
        let stressed = home.is_some() && rent > income * RENT_STRESS_SHARE;
        let stress = {
            let hh = &mut self.households[hid];
            hh.members = members.clone();
            hh.home = home;
            hh.money = money;
            hh.rent_stress_days = if stressed { hh.rent_stress_days.saturating_add(1) } else { 0 };
            hh.rent_stress_days
        };
        for &m in &members {
            let c = &mut self.citizens[m];
            c.rent_stress_days = stress;
            c.mood = (c.mood + if stressed { -0.05 } else { 0.01 }).clamp(0.0, 1.0);
        }
        if self.households[hid].moved_day == today && home.is_some() {
            return;
        }
        // Somewhere to live for those who lost theirs.
        let Some(home) = home else {
            match self.choose_home(income, None) {
                Some(b) => self.move_household(hid, b, "Found a place at {}."),
                None => self.economy.housing_demand_by_tier[Self::tier_affordable(income).index()] += 1.0,
            }
            return;
        };
        // Stressed: somewhere cheaper first, leaving town only when there is
        // nowhere cheaper and the money has run out.
        if stressed {
            match self.choose_home(income, Some((home, rent))) {
                Some(b) => {
                    self.move_household(hid, b, "Rent got too steep. Moved to {}.");
                    self.economy.displaced_total += members.len() as u32;
                }
                None => {
                    self.economy.housing_demand_by_tier[Self::tier_affordable(income).index()] += 1.0;
                    if stress >= 3 && money < -60.0 * members.len() as f32 {
                        for &m in &members {
                            self.citizens[m].remember(tick, "Couldn't make the rent anywhere in town. Leaving.");
                        }
                        // [society] Most stay on the street if there is cheap ground to camp on.
                        if !self.become_homeless(hid) {
                            self.emigrate_household(hid);
                        }
                    }
                }
            }
            return;
        }
        // Burdened but not yet stressed: take a cheaper place when one is free.
        if rent > income * AFFORDABLE_SHARE {
            if self.rng.random::<f32>() < 0.25 {
                if let Some(b) = self.choose_home(income, Some((home, rent))) {
                    if self.buildings[b].rent <= income * AFFORDABLE_SHARE {
                        self.move_household(hid, b, "Found somewhere cheaper: {}.");
                    }
                }
            }
            return;
        }
        // The comfortable move up a tier.
        if money > 300.0 && self.rng.random::<f32>() < 0.2 {
            let ct = self.buildings[home].tier;
            if ct < Self::tier_affordable(income) {
                let better = self.pick_building(|b| b.land_use == LandUse::Residential && b.construction.is_none() && b.tier > ct && b.free_homes() > 0 && b.rent <= income * AFFORDABLE_SHARE);
                if let Some(new_home) = better {
                    self.move_household(hid, new_home, "Moving up to {}.");
                }
            }
        }
    }

    /// The tier whose ordinary rent fits in `AFFORDABLE_SHARE` of `income`.
    pub(super) fn tier_affordable(income: f32) -> Tier {
        let budget = income * AFFORDABLE_SHARE;
        if budget >= Tier::High.base_rent() {
            Tier::High
        } else if budget >= Tier::Mid.base_rent() {
            Tier::Mid
        } else {
            Tier::Low
        }
    }

    /// A free home for a household earning `income`: the best tier whose
    /// rent is at most `AFFORDABLE_SHARE` of it (any building of that tier),
    /// failing that the cheapest one under `RENT_STRESS_SHARE`. With
    /// `cheaper_than`, only homes elsewhere with a lower rent count.
    pub(super) fn choose_home(&mut self, income: f32, cheaper_than: Option<(BuildingId, f32)>) -> Option<BuildingId> {
        let candidates: Vec<(BuildingId, Tier, f32)> = self
            .buildings
            .iter()
            .filter(|(id, b)| b.land_use == LandUse::Residential && b.construction.is_none() && b.free_homes() > 0 && cheaper_than.is_none_or(|(h, r)| *id != h && b.rent < r * 0.95))
            .filter(|(_, b)| b.rent <= income * RENT_STRESS_SHARE)
            .map(|(id, b)| (id, b.tier, b.rent))
            .collect();
        let comfortable: Vec<(BuildingId, Tier)> = candidates.iter().filter(|(_, _, r)| *r <= income * AFFORDABLE_SHARE).map(|(id, t, _)| (*id, *t)).collect();
        if let Some(best) = comfortable.iter().map(|(_, t)| *t).max() {
            let choices: Vec<BuildingId> = comfortable.iter().filter(|(_, t)| *t == best).map(|(id, _)| *id).collect();
            return choices.choose(&mut self.rng).copied();
        }
        candidates.iter().min_by(|a, b| a.2.total_cmp(&b.2)).map(|(id, _, _)| *id)
    }

    /// Move a whole household into `new_home`.
    pub(super) fn move_household(&mut self, hid: HouseholdId, new_home: BuildingId, template: &str) {
        let tick = self.clock.tick;
        let Some(hh) = self.households.get_mut(hid) else { return };
        let old = hh.home.replace(new_home);
        hh.rent_stress_days = 0;
        hh.moved_day = self.clock.day();
        let members = hh.members.clone();
        if let Some(b) = old.and_then(|o| self.buildings.get_mut(o)) {
            b.households.retain(|&h| h != hid);
            b.residents.retain(|c| !members.contains(c));
        }
        let b = &mut self.buildings[new_home];
        b.households.push(hid);
        b.residents.extend(members.iter().copied());
        let name = b.name.clone();
        for m in members {
            let Some(c) = self.citizens.get_mut(m) else { continue };
            c.home = Some(new_home);
            c.rent_stress_days = 0;
            c.remember(tick, template.replace("{}", &name));
            if matches!(c.activity, Activity::AtHome) {
                c.next_decision_tick = tick + 1;
            }
        }
    }

    pub(super) fn remove_household(&mut self, hid: HouseholdId) {
        if let Some(hh) = self.households.remove(hid) {
            if let Some(b) = hh.home.and_then(|h| self.buildings.get_mut(h)) {
                b.households.retain(|&h| h != hid);
            }
        }
    }

    /// The whole household leaves town.
    pub(super) fn emigrate_household(&mut self, hid: HouseholdId) {
        let members = self.households.get(hid).map(|h| h.members.clone()).unwrap_or_default();
        for m in members {
            self.emigrate(m);
        }
        self.remove_household(hid);
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
        if let Some(hid) = c.household {
            let empty = self.households.get_mut(hid).is_some_and(|h| {
                h.members.retain(|&m| m != cid);
                h.members.is_empty()
            });
            if empty {
                self.remove_household(hid);
            }
        }
        if let Activity::Driving { car, .. } = c.activity {
            self.traffic.remove_car(car);
        }
        self.economy.emigrated_total += 1;
        self.events.push(SimEvent::CitizenLeft(cid));
    }

    /// Households whose home was pulled down lose it here, and every
    /// member's `home` follows the household's. Also refreshes the pantry
    /// mirror (`Citizen::groceries`) on each member.
    pub(super) fn sync_households(&mut self) {
        for (hid, hh) in self.households.iter_mut() {
            if hh.home.is_some_and(|b| !self.buildings.get(b).is_some_and(|b| b.households.contains(&hid))) {
                hh.home = None;
            }
            for &m in &hh.members {
                if let Some(c) = self.citizens.get_mut(m) {
                    c.home = hh.home;
                    c.groceries = hh.groceries;
                }
            }
        }
    }

    /// Saves from before households: everyone becomes a household of one,
    /// with their savings as the purse and their groceries as the pantry.
    /// Also repairs citizens whose household went missing.
    pub(super) fn migrate_households(&mut self) {
        let ids: Vec<CitizenId> = self.citizens.keys().collect();
        for cid in ids {
            let ok = self.citizens[cid].household.is_some_and(|h| self.households.get(h).is_some_and(|hh| hh.members.contains(&cid)));
            if ok {
                continue;
            }
            let c = &mut self.citizens[cid];
            let home = c.home;
            let hh = Household { members: vec![cid], home, money: std::mem::take(&mut c.savings), groceries: c.groceries, rent_stress_days: c.rent_stress_days, moved_day: 0 };
            let hid = self.households.insert(hh);
            self.citizens[cid].household = Some(hid);
        }
        // A building lists exactly the households that live there.
        for b in self.buildings.values_mut() {
            b.households.retain(|h| self.households.contains_key(*h));
        }
        let homes: Vec<(HouseholdId, Option<BuildingId>)> = self.households.iter().map(|(id, h)| (id, h.home)).collect();
        for (hid, home) in homes {
            if let Some(b) = home.and_then(|h| self.buildings.get_mut(h)) {
                if !b.households.contains(&hid) {
                    b.households.push(hid);
                }
            }
        }
    }

    pub(super) fn entry_node(&mut self) -> Option<NodeId> {
        let leaves = self.city_leaves();
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
    /// employer first, food sellers (grocers, bakeries) well ahead of the
    /// rest and the farms, vats and mills that feed them next, so a growing
    /// town staffs its food chain before its sites and plants and nobody
    /// goes hungry beside full warehouses. `taken` are jobs already promised
    /// to others in the same household. Ties are broken at random so the
    /// same firm is not always picked.
    pub(super) fn pick_job_except(&mut self, taken: &[BuildingId]) -> Option<BuildingId> {
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
            .map(|(id, b)| (b.staffing() - bonus(b), id))
            .collect();
        let best = scored.iter().map(|(s, _)| *s).fold(f32::INFINITY, f32::min);
        let near_best: Vec<BuildingId> = scored.iter().filter(|(s, _)| *s <= best + 0.1).map(|(_, id)| *id).collect();
        near_best.choose(&mut self.rng).copied()
    }

    pub(super) fn pick_job(&mut self) -> Option<BuildingId> {
        self.pick_job_except(&[])
    }

    /// Households per arrival: one to three, plus more while homes and jobs
    /// stand open, so the town fills about as fast as it is built (a fixed
    /// trickle left a thousand homes empty and the shops unstaffed).
    /// While there is no work and a good share of the town's adults are
    /// already out of it, word gets round: households come only now and then.
    pub(super) fn arrivals(&mut self, free_homes: u32) -> u32 {
        let free_jobs: u32 = self.buildings.values().map(|b| b.free_jobs() as u32).sum();
        if free_jobs == 0 && self.economy.unemployment_share() > JOBLESS_SLOWS_ARRIVALS {
            return u32::from(self.rng.random_bool(0.3)).min(free_homes);
        }
        let pull = free_homes.min(free_jobs + 10) / 40;
        (self.rng.random_range(1..=3) + pull).min(MAX_ARRIVALS).min(free_homes)
    }

    /// The ages of a household arriving: singles, couples, retired
    /// couples, single parents and families with up to three children,
    /// sometimes with a grandparent.
    pub(super) fn household_ages(&mut self) -> Vec<u8> {
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

    pub(super) fn immigrate(&mut self) {
        let free_homes: u32 = self.buildings.values().map(|b| b.free_homes() as u32).sum();
        if free_homes == 0 {
            self.economy.waiting_immigrants = (self.economy.waiting_immigrants + 1).min(60);
            return;
        }
        let Some(entry) = self.entry_node() else { return };
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
            let Some(home) = self.choose_home(income, None) else {
                self.economy.priced_out_total += 1;
                self.economy.waiting_immigrants = (self.economy.waiting_immigrants + 1).min(60);
                self.economy.housing_demand_by_tier[Self::tier_affordable(income).index()] += 1.0;
                break;
            };
            let tick = self.clock.tick;
            let by_train = if !self.stations.is_empty() && self.rng.random_bool(0.6) { self.stations.choose(&mut self.rng).copied() } else { None };
            let pos = match by_train.and_then(|s| self.building_entrance(s)) {
                Some(p) => p,
                None => self.roads.node(entry).pos,
            };
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
            for (age, seed, job, income) in people {
                let stage = LifeStage::of_age(age);
                let name = names::member_name(&mut self.rng, &family);
                let owns_car = stage != LifeStage::Child && self.rng.random_bool(car_chance);
                let vehicle = owns_car.then(|| VehicleModel::for_owner(income, age, crate::vehicle::hedonism_of(seed), &mut crate::vehicle::model_rng(seed, 1)));
                let owns_bike = self.rng.random_bool(bike_chance);
                let savings = if stage == LifeStage::Child { 0.0 } else { self.rng.random_range(10.0..40.0) };
                let next_decision_tick = tick + self.rng.random_range(5..30);
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
                    vehicle,
                    parked_at: None,
                    transit: None,
                    activity: Activity::Idle,
                    memories: Vec::new(),
                    thought: "New city. New me.".to_string(),
                    spotlight: false,
                    next_decision_tick,
                    arrived_tick: tick,
                    inside: None,
                    mood: 0.6,
                    income,
                    savings,
                    rent_stress_days: 0,
                    satiety: 0.8,
                    groceries: 0.0,
                    hungry_days: 0,
                    trail: Vec::new(),
                    pending_footfall: Vec::new(),
                    household: Some(hid),
                    age,
                    wakefulness: 0.9,
                    asleep: false,
                    packed_meal: 1.0,
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
                self.travel_to(id, Some(home), Purpose::Home);
                self.events.push(SimEvent::CitizenArrived(id));
            }
            let groceries = self.households[hid].groceries;
            for &m in &self.households[hid].members {
                self.citizens[m].groceries = groceries;
            }
            self.economy.waiting_immigrants = self.economy.waiting_immigrants.saturating_sub(1);
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

    /// Adults without a job take an open one, food chain first.
    pub(super) fn job_search(&mut self) {
        let jobless: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.work.is_none() && c.can_work()).map(|(id, _)| id).collect();
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
        let hour = self.clock.hour() as u8;
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
                let hh = c.household.and_then(|h| self.households.get(h));
                let on_shift = c.work.and_then(|w| self.buildings.get(w)).is_some_and(|b| b.business.as_ref().is_none_or(|z| !z.closed) && b.shift_for(c.seed).contains(hour));
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
                    stage: c.stage(),
                    wakefulness: c.wakefulness,
                    asleep: c.asleep,
                    at_home: c.home.is_some() && c.inside == c.home,
                    on_shift,
                    household_size: hh.map_or(1, |h| h.size()),
                    pantry_low: hh.is_some_and(|h| h.groceries < h.pantry_target() * 0.5),
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
        // Shops may have shut, or filled up, on the way; a place to go out
        // to may have closed down or be packed (it lets late-comers in
        // after hours: last orders).
        if let (Purpose::Visit(_) | Purpose::Shop(_), Some(b)) = (purpose, to) {
            let hour = self.clock.hour() as u8;
            let bld = &self.buildings[b];
            let hours_ok = matches!(purpose, Purpose::Visit(_)) || bld.opening_hours().contains(hour);
            if !hours_ok || !bld.takes_customers() {
                let open = bld.opening_hours();
                let shut = !hours_ok || bld.business.as_ref().is_some_and(|z| z.closed) || bld.is_derelict() || bld.construction.is_some();
                let name = bld.name.clone();
                let c = &mut self.citizens[id];
                if shut {
                    c.remember(tick, format!("{name} was shut when I got there (open {})." , open.label()));
                    c.thought = "Shut. Should have checked the hours.".to_string();
                } else {
                    c.remember(tick, format!("{name} was packed. Couldn't get in."));
                    c.thought = "Packed to the doors. Another time.".to_string();
                }
                c.mood = (c.mood - 0.03).max(0.0);
                c.activity = Activity::Idle;
                c.inside = None;
                c.next_decision_tick = tick + 3;
                self.economy.market.turned_away_today += 1;
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
                        // The kitchen feeds the hungry from its groceries.
                        if c.satiety < 0.7 && bz.take(Resource::Groceries, 0.5) > 0.0 {
                            c.satiety = (c.satiety + 0.45).min(1.0);
                            self.economy.market.meals_sold_today += 1;
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
                // Somewhere that is open now and has room: not a site, not a
                // shut business, not a derelict shell, not packed.
                let hour = self.clock.hour() as u8;
                let target = self.pick_building(|b| b.land_use == lu && b.serves_at(hour));
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
        if fresh && purpose != Purpose::Home {
            self.pack_meal(id);
        }
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
        // The rich fly (`world/air.rs`): a drone taxi when pads serve both ends.
        if allow_bus {
            if let Some(plan) = self.consider_air(id, target, purpose, from_pt, entrance) {
                self.start_air_trip(id, plan, target, purpose, fresh);
                return;
            }
        }
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
                let model = {
                    let c = &mut self.citizens[id];
                    *c.vehicle.get_or_insert_with(|| VehicleModel::fallback(c.seed, c.income, c.age))
                };
                self.traffic.cars[car].model = model;
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

    /// Someone leaving home takes a meal from the pantry for later, if the
    /// pantry has one to spare.
    pub(super) fn pack_meal(&mut self, id: CitizenId) {
        let c = &self.citizens[id];
        if c.packed_meal >= 1.0 || c.home.is_none() || c.inside != c.home {
            return;
        }
        let Some(hh) = c.household.and_then(|h| self.households.get_mut(h)) else { return };
        if hh.groceries >= 1.0 {
            hh.groceries -= 1.0;
            let left = hh.groceries;
            let c = &mut self.citizens[id];
            c.packed_meal += 1.0;
            c.groceries = left;
        }
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

/// How a citizen names what they bought, for their memories.
fn model_name(m: &VehicleModel) -> String {
    let old = if m.class == VehicleClass::Cheap { "beat-up " } else { "" };
    let flash = if m.tuned { "souped-up " } else { "" };
    format!("{old}{flash}{}", m.body.label().replace('_', " "))
}
