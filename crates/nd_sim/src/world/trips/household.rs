//! Households day by day: income, rent, moving home, leaving town.

use super::*;

impl World {
    pub(in crate::world) fn daily_citizens(&mut self) {
        let cids: Vec<CitizenId> = self.citizens.keys().collect();
        for cid in cids {
            self.daily_citizen(cid);
        }
    }

    /// Once a day, per citizen: tax on the day's income, hunger days and
    /// what they do to mood. Four hungry days and they leave. Children left
    /// without a grown-up keep the home until the rent runs them out of
    /// money (`daily_household`), which is soon.
    pub(in crate::world) fn daily_citizen(&mut self, cid: CitizenId) {
        let tick = self.clock.tick;
        // [tax] Income tax at the rate of the home's tier.
        let cat = self.income_tax_category(cid);
        let rate = self.economy.taxes.rate(cat);
        let surcharge = self.economy.taxes.surcharge(cat);
        let Some(c) = self.citizens.get_mut(cid) else { return };
        let tax = c.income * rate;
        self.economy.tax_today += tax;
        self.economy.credit_item(Flow::IncomeTax, tax, Item::Class(c.class()));
        self.economy.taxes.collect(cat, tax);
        // [tax] A rate well over the standard is felt: a little mood a day,
        // and now and then a word about it.
        if surcharge > 0.02 && c.income > 0.0 {
            c.mood = (c.mood - surcharge * 0.25).max(0.0);
            if (tick / 1440 + super::cars::id_hash(cid)) % 5 == 0 {
                let pct = rate * 100.0;
                c.remember(tick, format!("{pct:.0}% of my pay goes to the city now. Hard to get ahead."));
                self.concerns.said(tick, cid, c, crate::concerns::Concern::Other, (surcharge * 3.0).clamp(0.2, 0.7), "Taxes eat half the paycheque.");
            }
        }
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
        self.emigrate(cid, crate::economy::LeaveReason::Hunger);
    }

    /// Everyone's daily earnings: wages, gig money or a pension.
    pub fn household_income(&self, hid: HouseholdId) -> f32 {
        self.households.get(hid).map_or(0.0, |h| h.members.iter().filter_map(|m| self.citizens.get(*m)).map(|c| c.income).sum())
    }

    /// Rent of the household's home over its income (infinite with no
    /// income), or `None` when it has no home.
    pub fn household_rent_share(&self, hid: HouseholdId) -> Option<f32> {
        let home = self.households.get(hid)?.home.filter(|b| self.buildings.contains_key(*b))?;
        let rent = self.rent_due(hid, home);
        let income = self.household_income(hid);
        Some(if income > 0.0 { rent / income } else { f32::INFINITY })
    }

    /// The rent share of the household a citizen belongs to.
    pub fn rent_share_of(&self, cid: CitizenId) -> Option<f32> {
        self.household_rent_share(self.citizens.get(cid)?.household?)
    }

    /// Once a day, per household: earnings in, rent and living costs out,
    /// pocket money for the grown-ups, wheels, rent stress and moving.
    pub(in crate::world) fn daily_household(&mut self, hid: HouseholdId) {
        let tick = self.clock.tick;
        let today = self.clock.day();
        let Some(hh) = self.households.get(hid) else { return };
        let members: Vec<CitizenId> = hh.members.iter().copied().filter(|m| self.citizens.contains_key(*m)).collect();
        if members.is_empty() {
            self.remove_household(hid);
            return;
        }
        let home = hh.home.filter(|h| self.buildings.contains_key(*h));
        // [megacorp] Staff in company housing and students in dorms pay less.
        let rent = home.map(|h| self.rent_due(hid, h)).unwrap_or(0.0);
        let income = self.household_income(hid);
        let living: f32 = members.iter().map(|m| if self.citizens[*m].stage() == LifeStage::Child { 10.0 } else { 20.0 }).sum();
        // [tax] Wages are what is left after the standard income tax; a
        // rate over it comes out of the purse, one under it is kept.
        let tax_extra = income * self.economy.taxes.surcharge(self.tax_category_of_home(home));
        // Pockets come back to the shared purse (with any debts run up on
        // them), then the grown-ups draw pocket money again if there is some.
        let mut money = self.households[hid].money + income - rent - living - tax_extra;
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
            match self.choose_home(income, None, self.has_children(hid)) {
                Some(b) => self.move_household(hid, b, "Found a place at {}."),
                None => self.economy.housing_demand_by_tier[Self::tier_affordable(income).index()] += 1.0,
            }
            return;
        };
        // Stressed: somewhere cheaper first, leaving town only when there is
        // nowhere cheaper and the money has run out.
        if stressed {
            match self.choose_home(income, Some((home, rent)), self.has_children(hid)) {
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
                            self.emigrate_household(hid, crate::economy::LeaveReason::Rent);
                        }
                    }
                }
            }
            return;
        }
        // Burdened but not yet stressed: take a cheaper place when one is free.
        if rent > income * AFFORDABLE_SHARE {
            if self.rng.random::<f32>() < 0.25 {
                if let Some(b) = self.choose_home(income, Some((home, rent)), self.has_children(hid)) {
                    if self.buildings[b].rent <= income * AFFORDABLE_SHARE {
                        self.move_household(hid, b, "Found somewhere cheaper: {}.");
                    }
                }
            }
            return;
        }
        // [megacorp] A family out of reach of any school moves near one.
        if self.family_school_move(hid, income) {
            return;
        }
        // The comfortable move up a tier.
        if money > 300.0 && self.rng.random::<f32>() < 0.2 {
            let ct = self.buildings[home].tier;
            if ct < Self::tier_affordable(income) {
                let better = self.pick_building(|b| b.land_use == LandUse::Residential && b.operating() && b.tier > ct && b.free_homes() > 0 && b.rent <= income * AFFORDABLE_SHARE);
                if let Some(new_home) = better {
                    self.move_household(hid, new_home, "Moving up to {}.");
                }
            }
        }
    }

    /// The tier whose ordinary rent fits in `AFFORDABLE_SHARE` of `income`.
    pub(in crate::world) fn tier_affordable(income: f32) -> Tier {
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
    /// [megacorp] A family (`family`: children in the household) takes one a
    /// school covers when its tier has one free; dorms and frat houses are
    /// the university's.
    pub(in crate::world) fn choose_home(&mut self, income: f32, cheaper_than: Option<(BuildingId, f32)>, family: bool) -> Option<BuildingId> {
        let candidates: Vec<(BuildingId, Tier, f32)> = self
            .buildings
            .iter()
            .filter(|(id, b)| b.land_use == LandUse::Residential && b.operating() && b.free_homes() > 0 && b.annex.is_none() && cheaper_than.is_none_or(|(h, r)| *id != h && b.rent < r * 0.95))
            .filter(|(_, b)| b.rent <= income * RENT_STRESS_SHARE)
            .map(|(id, b)| (id, b.tier, b.rent))
            .collect();
        let comfortable: Vec<(BuildingId, Tier)> = candidates.iter().filter(|(_, _, r)| *r <= income * AFFORDABLE_SHARE).map(|(id, t, _)| (*id, *t)).collect();
        if let Some(best) = comfortable.iter().map(|(_, t)| *t).max() {
            let choices: Vec<BuildingId> = comfortable.iter().filter(|(_, t)| *t == best).map(|(id, _)| *id).collect();
            let choices = if family { self.family_homes(choices) } else { choices };
            return choices.choose(&mut self.rng).copied();
        }
        candidates.iter().min_by(|a, b| a.2.total_cmp(&b.2)).map(|(id, _, _)| *id)
    }

    /// Move a whole household into `new_home`.
    pub(in crate::world) fn move_household(&mut self, hid: HouseholdId, new_home: BuildingId, template: &str) {
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

    pub(in crate::world) fn remove_household(&mut self, hid: HouseholdId) {
        if let Some(hh) = self.households.remove(hid) {
            if let Some(b) = hh.home.and_then(|h| self.buildings.get_mut(h)) {
                b.households.retain(|&h| h != hid);
            }
        }
    }

    /// The whole household leaves town.
    pub(in crate::world) fn emigrate_household(&mut self, hid: HouseholdId, why: crate::economy::LeaveReason) {
        let members = self.households.get(hid).map(|h| h.members.clone()).unwrap_or_default();
        for m in members {
            self.emigrate(m, why);
        }
        self.remove_household(hid);
    }

    pub(in crate::world) fn emigrate(&mut self, cid: CitizenId, why: crate::economy::LeaveReason) {
        if self.remove_citizen(cid) {
            self.economy.emigrated_total += 1;
            self.economy.emigrated_by[why.index()] += 1;
        }
    }

    /// Take a citizen out of the city (with their car), from wherever they
    /// are; false when there was no such citizen. Not counted as leaving.
    pub(in crate::world) fn remove_citizen(&mut self, cid: CitizenId) -> bool {
        if !self.citizens.contains_key(cid) {
            return false;
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
        let Some(c) = self.citizens.remove(cid) else { return false };
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
        self.events.push(SimEvent::CitizenLeft(cid));
        true
    }

    /// Households whose home was pulled down lose it here, and every
    /// member's `home` follows the household's. Also refreshes the pantry
    /// mirror (`Citizen::groceries`) on each member.
    pub(in crate::world) fn sync_households(&mut self) {
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
    pub(in crate::world) fn migrate_households(&mut self) {
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
    pub(in crate::world) fn migrate_looks(&mut self) {
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

    /// [carlife] Once a day per grown-up: buy or sell wheels by how
    /// travelling has felt (`Commute`) and what the household can afford.
    pub(in crate::world) fn wheels(&mut self, m: CitizenId, money: &mut f32, today: u64) {
        let tick = self.clock.tick;
        let Some(c) = self.citizens.get(m) else { return };
        let class = c.class();
        let owns_bike = c.owns_bike;
        let has_car = c.car.is_some();
        let p = &c.commute;
        let (walk_p, bike_p, car_p, park_p) = (p.pain_of(TripMode::Walk), p.pain_of(TripMode::Bike), p.pain_of(TripMode::Car), p.parking_pain);
        let transit_p = p.pain_of(TripMode::Bus).max(p.pain_of(TripMode::Train)).max(p.pain_of(TripMode::Subway));
        let transit_used = p.trips[TripMode::Bus.index()] + p.trips[TripMode::Train.index()] + p.trips[TripMode::Subway.index()] > 0.3;
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
}
