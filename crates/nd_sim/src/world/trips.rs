//! Citizens: daily upkeep, immigration and emigration, jobs, decisions,
//! journeys (`travel_to`, `arrive`, `wander`) and footfall.

use super::*;
use crate::citizens::{Household, HouseholdId, LifeStage, TripMode, GROCERIES_PER_PERSON_DAY, PENSION, POCKET_MONEY};
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
            // [concerns]
            let days = c.hungry_days;
            let text = if days > 1 { format!("Hungry for {days} days now. No proper meal.") } else { "Went to bed hungry.".to_string() };
            self.concerns.said(tick, cid, c, crate::concerns::Concern::Food, 0.3 + 0.15 * days as f32, text);
        } else {
            c.hungry_days = 0;
        }
        // [concerns] Grown-ups on gig money a day after arriving.
        if c.can_work() && c.work.is_none() && tick.saturating_sub(c.arrived_tick) > 1440 {
            self.concerns.said(tick, cid, c, crate::concerns::Concern::Jobs, 0.3, "Still no steady work. Gigs don't pay the bills.");
        }
        if c.hungry_days < 4 {
            return;
        }
        c.remember(tick, "Couldn't keep myself fed here. Leaving.".to_string());
        self.concerns.heard(cid, c, crate::concerns::Concern::Food, 0.9);
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
        // [carlife] Wheels: habits follow how travelling has felt. Long
        // walks buy a bike, parking misery or jams sell the car, transit
        // that keeps failing buys one (if it can be afforded); the poor
        // sell a car they cannot keep. Cars come from a dealer and are
        // delivered; sold ones are driven away.
        for &m in &grown {
            self.wheels(m, &mut money, today);
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
            // [concerns]
            if stressed && c.stage() != LifeStage::Child {
                self.concerns.said(tick, m, c, crate::concerns::Concern::Housing, 0.35 + 0.1 * stress.min(5) as f32, "Rent takes more than half of what we bring in.");
            }
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
                            self.concerns.heard(m, &self.citizens[m], crate::concerns::Concern::Housing, 0.9);
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
        // [carlife] The car leaves town with its owner.
        self.car_emigrates(cid);
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
        self.migrate_looks();
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

    /// [looks] Saves from before looks: each household's members get the
    /// looks they would have been given on arrival (children resembling
    /// their parents). Members who already have one keep it.
    pub(super) fn migrate_looks(&mut self) {
        let mut fill: Vec<(CitizenId, crate::looks::Appearance)> = Vec::new();
        for hh in self.households.values() {
            let members: Vec<CitizenId> = hh.members.iter().copied().filter(|m| self.citizens.contains_key(*m)).collect();
            if members.iter().all(|m| self.citizens[*m].look.is_some()) {
                continue;
            }
            let earners = members.iter().filter(|m| self.citizens[**m].income > 0.0).count().max(1) as f32;
            let income: f32 = members.iter().map(|m| self.citizens[*m].income).sum();
            let ids: Vec<(u64, u8)> = members.iter().map(|m| (self.citizens[*m].seed, self.citizens[*m].age)).collect();
            let looks = crate::looks::family_looks(&ids, Tier::for_income(income / earners));
            fill.extend(members.into_iter().zip(looks));
        }
        for (id, look) in fill {
            let c = &mut self.citizens[id];
            if c.look.is_none() {
                c.look = Some(look);
            }
        }
        // Anyone still without (no household): their own look.
        for c in self.citizens.values_mut() {
            if c.look.is_none() {
                c.look = Some(c.appearance());
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
            let Some(home) = self.choose_home(income, None) else {
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
                    places: Default::default(),
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
                    drunk: 0.0,
                    car: None,
                    to_car: None,
                    commute: Default::default(),
                    look: Some(look),
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
        // [pedestrians] Walkers keep right, keep their distance and mind the
        // crossings (`world/pedestrians.rs`).
        let mut feet = self.feet_begin();

        for id in ids {
            // Walking.
            let on_foot = self.citizens[id].mode == TravelMode::Walk && matches!(self.citizens[id].activity, Activity::Traveling { .. });
            let arrived = if on_foot {
                self.step_walker(id, &mut feet)
            } else {
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
                    regular_spot: c.places.regular_of(LandUse::Entertainment).and_then(|p| self.buildings.get(p.place)).filter(|b| b.serves_at(hour)).map(|b| b.name.as_str()),
                    places_known: c.places.known(LandUse::Entertainment),
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
        self.feet_end(feet);
    }

    pub(super) fn arrive(&mut self, id: CitizenId) {
        let tick = self.clock.tick;
        let (to, purpose) = match &self.citizens[id].activity {
            Activity::Traveling { to, purpose, .. } | Activity::Driving { to, purpose, .. } => (*to, *purpose),
            _ => return,
        };
        // [carlife] Reached the parked car: drive on from here. If it
        // cannot get there after all, go on some other way.
        // [parking] Only where the car is (a pound trip may pass a bus stop).
        let at_car = self.citizens[id].to_car.and_then(|tc| self.vehicles.cars.get(tc.car)).and_then(|c| c.spot()).is_none_or(|s| s.pos.distance(self.citizens[id].pos) < 60.0 || s.kind != crate::vehicle::ParkKind::Pound);
        if at_car && matches!(self.citizens[id].activity, Activity::Traveling { .. }) {
            if let Some(tc) = self.citizens[id].to_car.take() {
                if !self.start_drive(id, tc.car, tc.to, tc.purpose) {
                    self.travel_with(id, Some(tc.to), tc.purpose, true, false, false);
                }
                return;
            }
        }
        // [carlife] The journey is over (a stop is a pause in it): how did
        // it feel?
        if purpose != Purpose::ToStop {
            self.end_trip(id);
        }
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
                self.concerns.heard(id, c, crate::concerns::Concern::Nightlife, 0.2);
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
                    self.concerns.said(tick, id, c, crate::concerns::Concern::Jobs, 0.3, "Turned away at the door. Skint.");
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
                self.rate_visit(id, b);
                return;
            }
            _ => {
                c.activity = Activity::Idle;
                c.inside = None;
            }
        }
        c.next_decision_tick = tick + 30 + self.rng.random_range(0..90);
        // [places] A night out, an afternoon in the park: how was it?
        match (purpose, &self.citizens[id].activity) {
            (Purpose::Visit(_), Activity::Visiting(b)) | (Purpose::Sit, Activity::Sitting { park: b, .. }) => {
                let b = *b;
                self.rate_visit(id, b);
            }
            _ => {}
        }
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
                // [places] Favourites, sometimes somewhere new, never a
                // place sworn off; sometimes with company.
                let target = self.choose_place(id, lu, false);
                if let Some(b) = target {
                    if inside != Some(b) {
                        self.travel_to(id, Some(b), Purpose::Visit(lu));
                        self.bring_company(id, b, Purpose::Visit(lu));
                    }
                }
            }
            Intent::Explore(lu) => match self.choose_place(id, lu, true) {
                Some(b) if inside != Some(b) => self.travel_to(id, Some(b), Purpose::Visit(lu)),
                Some(_) => {}
                None => self.wander(id),
            },
            Intent::Wander => self.wander(id),
            Intent::Shop(r) => {
                let here = self.citizens[id].pos;
                if let Some(b) = self.choose_shop(id, r, here) {
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
        self.travel_with(id, target, purpose, allow_bus, fresh, true);
    }

    /// [carlife] `travel_to_impl`, optionally without the car (it was not
    /// where the citizen walked to). The mode is the citizen's choice
    /// (`choose_mode`); if the best one cannot be done after all (no route
    /// for the car), the next best is taken.
    pub(super) fn travel_with(&mut self, id: CitizenId, target: Option<BuildingId>, purpose: Purpose, allow_bus: bool, fresh: bool, allow_car: bool) {
        let Some(target) = target else { return };
        self.citizens[id].transit = None;
        self.citizens[id].to_car = None;
        if fresh && purpose != Purpose::Home {
            self.pack_meal(id);
        }
        if fresh {
            self.citizens[id].pending_footfall.clear();
        }
        let Some((_from_seg, from_pt, from_node)) = self.origin_for(id) else {
            self.citizens[id].activity = Activity::Idle;
            return;
        };
        let Some(lot) = self.building_lot(target) else { return };
        let to_node = lot.access_node;
        let entrance = lot.entrance();
        let seated = matches!(self.citizens[id].activity, Activity::Sitting { .. });
        let walk_from = if self.citizens[id].inside.is_some() { from_pt } else { self.citizens[id].pos };
        // [air] The rich fly when pads serve both ends and a drone beats
        // the drive (`world/air.rs` decides); everyone else ranks the
        // ground modes below. Drone trips keep their own statistics.
        if allow_bus {
            if let Some(plan) = self.consider_air(id, target, purpose, walk_from, entrance) {
                self.start_air_trip(id, plan, target, purpose, fresh);
                return;
            }
        }
        let q = TripQuery { from: walk_from, to: entrance, target, purpose, allow_transit: allow_bus, allow_car, allow_bike: !seated };
        let ranked = self.choose_mode(id, &q);
        // [taxis] A trip that would have gone by cab had one been in reach
        // is the demand a taxi company opens on.
        if fresh {
            self.note_taxi_demand(id, &q, &ranked);
        }
        let dist = walk_from.distance(entrance);
        for opt in ranked {
            match opt.plan {
                ModePlan::Car { car, at } => {
                    if at.distance(walk_from) > 25.0 {
                        self.walk_to_car(id, car, at, target, purpose, walk_from);
                    } else if !self.start_drive(id, car, target, purpose) {
                        continue;
                    }
                }
                ModePlan::Train { board, alight, stop } => self.walk_to_transit(id, stop, TransitLeg::Rail { board, alight }, target, purpose, fresh),
                ModePlan::Bus { route, board, alight, stop } => self.walk_to_transit(id, stop, TransitLeg::Bus { route, board, alight }, target, purpose, fresh),
                ModePlan::Taxi { segment, at, wait_at } => self.hail_taxi(id, segment, at, wait_at, target, purpose, fresh),
                ModePlan::Walk | ModePlan::Bike => {
                    let mode = if matches!(opt.plan, ModePlan::Bike) { TravelMode::Bike } else { TravelMode::Walk };
                    if !self.walk_or_ride(id, target, purpose, mode, from_node, to_node, walk_from, entrance, fresh) {
                        continue;
                    }
                }
            }
            if fresh {
                self.begin_trip(id, opt.mode, dist);
            }
            return;
        }
        // Nothing gets there.
        if let Some(exit) = self.leave_building(id) {
            self.citizens[id].pos = exit;
        }
        let c = &mut self.citizens[id];
        c.thought = "No way through. The grid doesn't connect.".to_string();
        c.activity = Activity::Idle;
        let tick = self.clock.tick;
        self.concerns.said(tick, id, &self.citizens[id], crate::concerns::Concern::Commute, 0.5, "No way through. The grid doesn't connect.");
    }

    /// On foot or by bike along the streets (walkers through the blocks).
    #[allow(clippy::too_many_arguments)]
    fn walk_or_ride(&mut self, id: CitizenId, target: BuildingId, purpose: Purpose, mode: TravelMode, from_node: NodeId, to_node: NodeId, walk_from: Vec2, entrance: Vec2, fresh: bool) -> bool {
        let Some(nodes) = self.roads.path(from_node, to_node) else { return false };
        let exit_pos = self.leave_building(id);
        let mut path = match mode {
            TravelMode::Walk => match self.walk_route_for(id, walk_from, entrance, purpose) {
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
        true
    }

    /// [carlife] Every way this citizen could make this trip, with what
    /// each would cost them (`ModeOption`). Extend here for a new mode (air
    /// taxis, say): push an option with its own `ModePlan`.
    pub fn mode_options(&self, id: CitizenId, q: &TripQuery) -> Vec<ModeOption> {
        let Some(c) = self.citizens.get(id) else { return Vec::new() };
        let dist = q.from.distance(q.to);
        let secs = self.config.traffic_seconds_per_tick;
        let walk = self.config.walk_speed.max(0.1);
        let bike = self.config.bike_speed.max(0.1);
        let [sociable, ambition, hedonism, caution, curiosity] = crate::vehicle::traits_of(c.seed);
        let frail = if matches!(c.stage(), LifeStage::Child | LifeStage::Retiree) { 1.5 } else { 1.0 };
        let time_w = 0.7 + 0.6 * ambition;
        let money_w = self.money_weight(id);
        let mut out = Vec::new();
        let mut push = |mode: TripMode, minutes: f32, money: f32, comfort: f32, plan: ModePlan| {
            let pain = c.commute.pain_of(mode);
            let cost = minutes * time_w + money * money_w + comfort + PAIN_WEIGHT * pain;
            out.push(ModeOption { mode, minutes, money, comfort, pain, cost, plan });
        };
        // On foot: always possible, tiring when long.
        let minutes = dist * 1.25 / walk;
        push(TripMode::Walk, minutes, 0.0, (minutes - 15.0).max(0.0) * 0.35 * frail * (1.2 - 0.5 * curiosity), ModePlan::Walk);
        // A bike, for owners: quick, a little sweaty, risky to the cautious.
        let park_ride = matches!(q.purpose, Purpose::Visit(LandUse::Park));
        if c.owns_bike && q.allow_bike && (park_ride || dist >= self.config.bike_threshold) {
            let minutes = dist * 1.2 / bike + 2.0;
            // Riding to the park is the outing itself.
            let comfort = if park_ride { -30.0 } else { minutes * 0.25 * (0.5 + caution) * frail + 4.0 };
            push(TripMode::Bike, minutes, 0.0, comfort, ModePlan::Bike);
        }
        // The bus and the train, for longer trips: the walk to the stop,
        // the wait, the ride, the fare; the unsociable mind the crowd.
        let crowd = 6.0 - 6.0 * sociable;
        // Only while the buses run: at night a rider would stand at the
        // stop until morning (or give up hours later).
        if q.allow_transit && self.economy.transit.buses > 0 && crate::transit::Transit::in_service(self.clock.minute_of_day() as u32) {
            if let Some((route, board, alight)) = self.transit.plan_trip(q.from, q.to) {
                let r = &self.transit.routes[route];
                let (s0, s1) = (r.stops[board].pos, r.stops[alight].pos);
                let minutes = (q.from.distance(s0) + s1.distance(q.to)) / walk + 12.0 + r.ride_length(board, alight) / (7.0 * secs);
                push(TripMode::Bus, minutes, r.fare, crowd, ModePlan::Bus { route, board, alight, stop: s0 });
            }
        }
        if q.allow_transit && Rail::in_service(self.clock.minute_of_day() as u32) {
            if let Some((board, alight, stop)) = self.plan_rail_trip(q.from, q.to) {
                let end = self.building_entrance(alight).unwrap_or(q.to);
                let wait = match self.rail.stats.next_passenger_in {
                    0 => 30.0,
                    n => (n as f32).min(120.0),
                };
                let minutes = (q.from.distance(stop) + end.distance(q.to)) / walk + wait + dist / (15.0 * secs);
                push(TripMode::Train, minutes, rail::RAIL_FARE, crowd, ModePlan::Train { board, alight, stop });
            }
        }
        // The car, when it is parked nearby and the trip is worth it: the
        // walk to it, the drive, finding a space (as remembered), fuel and
        // the fee at the far end. A car is a comfortable seat out of the
        // weather; hedonists like driving most.
        // [driving] A drunk leaves the car, unless rebellious enough to drive
        // anyway (`citizens::wants_to_drive_drunk`), and then takes it even
        // for a short hop.
        let drunk_drive = crate::citizens::wants_to_drive_drunk(c);
        let too_drunk = c.drunk >= crate::citizens::DRUNK && !drunk_drive;
        if q.allow_car && !too_drunk && (dist >= self.config.drive_threshold || drunk_drive) && self.building_lot(q.target).is_some_and(|l| l.car_segment.is_some()) {
            if let Some((car, at)) = self.car_ready(id, q.from) {
                let own_space = self.buildings.get(q.target).is_some_and(|b| !b.is_parking() && b.parking_free() > 0);
                let fee = if own_space {
                    0.0
                } else {
                    self.buildings.values().filter(|b| b.is_parking() && b.parking_free() > 0).filter_map(|b| self.lots.get(b.lot).map(|l| (l.center.distance(q.to), b.parking_price))).filter(|(d, _)| *d < 180.0).map(|(_, p)| p).fold(f32::INFINITY, f32::min)
                };
                let fee = if fee.is_finite() { fee * 0.5 } else { 0.0 };
                let search = if own_space { 0.0 } else { 2.0 + c.commute.parking_pain * 10.0 };
                let minutes = at.distance(q.from) / walk + dist * 1.3 / (9.0 * secs) + 3.0 + search;
                // [taxis] With a cab to be had, fewer drunks risk the wheel.
                let bravado = if !drunk_drive {
                    0.0
                } else if self.taxis_in_service() {
                    -15.0
                } else {
                    -40.0
                };
                push(TripMode::Car, minutes, dist / 1000.0 * 0.5 + fee, -4.0 - 8.0 * hedonism + bravado, ModePlan::Car { car, at });
            }
        }
        // [taxis] A cab, when a company has one on duty within reach.
        if q.allow_transit {
            if let Some(o) = self.taxi_option(id, q, false) {
                out.push(o);
            }
        }
        out
    }

    /// [carlife] The options ranked best first, with a little day-to-day
    /// whim so habits are not clockwork.
    pub fn choose_mode(&mut self, id: CitizenId, q: &TripQuery) -> Vec<ModeOption> {
        let mut options = self.mode_options(id, q);
        for o in &mut options {
            o.cost *= self.rng.random_range(0.85..1.15f32);
        }
        options.sort_by(|a, b| a.cost.total_cmp(&b.cost));
        options
    }

    /// [carlife] Once a day per grown-up: buy or sell wheels by how
    /// travelling has felt (`Commute`) and what the household can afford.
    pub(super) fn wheels(&mut self, m: CitizenId, money: &mut f32, today: u64) {
        let tick = self.clock.tick;
        let Some(c) = self.citizens.get(m) else { return };
        let class = c.class();
        let owns_bike = c.owns_bike;
        let has_car = c.car.is_some();
        let p = &c.commute;
        let (walk_p, bike_p, car_p, park_p) = (p.pain_of(TripMode::Walk), p.pain_of(TripMode::Bike), p.pain_of(TripMode::Car), p.parking_pain);
        let transit_p = p.pain_of(TripMode::Bus).max(p.pain_of(TripMode::Train));
        let transit_used = p.trips[TripMode::Bus.index()] + p.trips[TripMode::Train.index()] > 0.3;
        let cheap_car = self.citizens[m].car.and_then(|v| self.vehicles.cars.get(v)).is_some_and(|v| v.model.class == VehicleClass::Cheap);
        let wants = VehicleModel::for_owner(c.income, c.age, crate::vehicle::hedonism_of(c.seed), &mut crate::vehicle::model_rng(c.seed, today + 2));
        // A car the poor cannot keep.
        if has_car && class == Tier::Low && *money < -100.0 {
            if let Some(got) = self.sell_car(m, "Sold the car. Couldn't afford to keep it.") {
                *money += got.min(150.0);
            }
            return;
        }
        // Parking misery or endless jams, and another way to get about.
        if has_car && (park_p > 1.5 || car_p > 2.5) && (owns_bike || transit_used || walk_p < 1.0) && self.rng.random_bool(0.25) {
            let reason = if park_p >= car_p * 0.6 { "Sick of circling for a space every day. Sold the car." } else { "Hours in traffic, every single day. Sold the car." };
            if let Some(got) = self.sell_car(m, reason) {
                *money += got;
            }
            return;
        }
        // The rich trade an old wreck up.
        if has_car && class == Tier::High && *money > 1500.0 && cheap_car && self.rng.random_bool(0.05) {
            if let Some(got) = self.sell_car(m, "Traded the old wreck in.") {
                *money += got;
                if let Some(paid) = self.buy_car(m, wants, wants.price().min(800.0)) {
                    *money -= paid;
                }
            }
            return;
        }
        // Bikes: after long walks, or just because.
        if !owns_bike {
            if walk_p > 1.0 {
                if *money > crate::vehicle::BIKE_PRICE + 15.0 && self.rng.random_bool(0.5) {
                    *money -= self.buy_bike(m, "Tired of the long walks.");
                }
            } else if *money > 60.0 && self.rng.random_bool(0.05) {
                *money -= self.buy_bike(m, "The city just got smaller.");
            }
        }
        // A car: when buses and trains keep failing, or the walks and rides
        // are too long, and it can be afforded; now and then regardless.
        if !has_car {
            let afford = (class != Tier::Low && *money > 400.0) || (class == Tier::Low && *money > 150.0);
            let fed_up = transit_p > 1.2 || (walk_p > 1.5 && (!self.citizens[m].owns_bike || bike_p > 1.2));
            let chance = if fed_up { 0.4 } else if class != Tier::Low { 0.04 } else { 0.02 };
            if afford && self.rng.random_bool(chance) {
                let price = wants.price().min(if class == Tier::Low { 150.0 } else { 300.0 });
                if let Some(paid) = self.buy_car(m, wants, price) {
                    *money -= paid;
                    let c = &mut self.citizens[m];
                    if fed_up {
                        c.remember(tick, if transit_p > 1.2 { "Transit let me down once too often. Got a car." } else { "Too far to walk, too far to ride. Got a car." });
                    }
                    // The bike goes if riding it is what hurt.
                    if c.owns_bike && bike_p > 1.5 {
                        c.owns_bike = false;
                        *money += 15.0;
                        c.remember(tick, "Sold the bike.");
                        self.vehicles.stats.bikes_sold_today += 1;
                    }
                }
            }
        }
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
            if let Some(park) = self.choose_place(id, LandUse::Park, false) {
                self.travel_to(id, Some(park), Purpose::Visit(LandUse::Park));
                return;
            }
        }
        let mode = if owns_bike && self.rng.random_bool(0.5) { TravelMode::Bike } else { TravelMode::Walk };
        let here = self.citizens[id].pos;
        let walk_route = if mode == TravelMode::Walk && !self.walk.nodes.is_empty() {
            let target = self.walk.nodes[self.rng.random_range(0..self.walk.nodes.len())].pos;
            self.walk_route_for(id, here, target, Purpose::Wander)
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

/// What a trip asks of the mode choice: from where to where, for what,
/// and which modes are allowed this time.
#[derive(Clone, Copy, Debug)]
pub struct TripQuery {
    pub from: Vec2,
    pub to: Vec2,
    pub target: BuildingId,
    pub purpose: Purpose,
    pub allow_transit: bool,
    pub allow_car: bool,
    pub allow_bike: bool,
}

/// How a trip would be made, concretely.
#[derive(Clone, Copy, Debug)]
pub enum ModePlan {
    Walk,
    Bike,
    Bus { route: RouteId, board: usize, alight: usize, stop: Vec2 },
    Train { board: BuildingId, alight: BuildingId, stop: Vec2 },
    /// The citizen's car, parked at `at`.
    Car { car: crate::vehicle::VehicleId, at: Vec2 },
    /// [taxis] Hail a cab: wait at `wait_at` for one coming to `at` on
    /// `segment` (`world/taxis.rs`).
    Taxi { segment: SegmentId, at: Vec2, wait_at: Vec2 },
}

/// One way to make a trip and what it would cost the traveller, in
/// minute-equivalents: `minutes` weighed by ambition, `money` by income,
/// `comfort` by personality (negative is a pleasure), and the remembered
/// `pain` of the mode times `PAIN_WEIGHT`.
#[derive(Clone, Copy, Debug)]
pub struct ModeOption {
    pub mode: TripMode,
    pub minutes: f32,
    pub money: f32,
    pub comfort: f32,
    pub pain: f32,
    pub cost: f32,
    pub plan: ModePlan,
}

/// Minute-equivalents one remembered bad half-hour adds to a mode.
pub const PAIN_WEIGHT: f32 = 20.0;
