//! [land] The super-elite: billionaires arriving, their mansions, guests,
//! cars, and the NIMBY round their homes.

use super::*;

impl World {
    /// The disparity a billionaire needs: (population, Gini, top twentieth
    /// over the median), and whether it is extreme enough.
    pub fn elite_gate(&self) -> (usize, f32, f32, bool) {
        let pop = self.citizens.len();
        let mut incomes: Vec<f32> = self.citizens.values().filter(|c| c.stage() != crate::citizens::LifeStage::Child).map(|c| c.income).collect();
        incomes.sort_by(|a, b| a.total_cmp(b));
        let ratio = if incomes.len() >= 20 {
            let median = incomes[incomes.len() / 2].max(1.0);
            let top = &incomes[incomes.len() - incomes.len() / 20..];
            top.iter().sum::<f32>() / top.len() as f32 / median
        } else {
            0.0
        };
        let gini = self.economy.gini;
        (pop, gini, ratio, pop >= ELITE_MIN_POP && gini >= ELITE_MIN_GINI && ratio >= ELITE_MIN_TOP_RATIO)
    }

    pub fn elite_cap(&self) -> usize {
        (self.citizens.len() / ELITE_PEOPLE_EACH).clamp(1, ELITE_MAX)
    }

    pub(in crate::world) fn daily_elites(&mut self) {
        // Their fortune pays them whatever job they took; the supercar is
        // brought home for the morning.
        for i in 0..self.estate.elites.len() {
            let e = &self.estate.elites[i];
            let (cid, income) = (e.citizen, elite_income(e.net_worth));
            if let Some(c) = self.citizens.get_mut(cid) {
                c.income = c.income.max(income);
            }
            if let Some(at) = self.citizens.get(cid).and_then(|c| c.home).and_then(|h| self.building_entrance(h)) {
                self.bring_car(cid, at);
            }
        }
        if self.estate.elites.len() >= self.elite_cap() || !self.elite_gate().3 {
            return;
        }
        let mut rng = self.erng(erng_salt::ELITES);
        let chance = (ELITE_DAILY_CHANCE * (1.0 + 0.5 * self.estate.elites.len() as f64)).min(0.25);
        if rng.random_bool(chance) {
            let _ = self.spawn_elite(&mut rng);
        }
    }

    /// A billionaire buys a quiet cluster of residential plots and builds a
    /// mansion; a local made rich, or a visitor who stays. Their other
    /// homes, businesses and cars come with them. Errors when there is no
    /// quiet land or nobody to be one. (Called by the daily roll once the
    /// disparity holds, and by the remote API's dev command.)
    pub fn spawn_elite(&mut self, rng: &mut SimRng) -> Result<CitizenId, String> {
        if self.estate.elites.len() >= ELITE_MAX {
            return Err("the town has all the billionaires it can carry".into());
        }
        let site = self.elite_site_why()?;
        let worth = 10f64.powf(9.0 + rng.random::<f64>() * 1.65);
        // A local who made it, else a visitor who stayed.
        let local = self
            .citizens
            .iter()
            .filter(|(id, c)| c.stage() == crate::citizens::LifeStage::Adult && c.class() == Tier::High && c.household.is_some() && !self.estate.is_elite(*id))
            .max_by(|a, b| a.1.income.total_cmp(&b.1.income).then(b.0.cmp(&a.0)))
            .map(|(id, _)| id);
        let cid = match local.filter(|_| rng.random_bool(0.5)) {
            Some(c) => c,
            None => self.elite_visitor(rng).or(local).ok_or("nobody to be one")?,
        };
        let origin = if local == Some(cid) { EliteOrigin::Local } else { EliteOrigin::Visitor };
        let name = self.citizens[cid].name.clone();
        let owner = self.new_owner(name.clone(), OwnerKind::Elite { citizen: cid });
        let income = elite_income(worth);
        let tick = self.clock.tick;
        {
            let c = &mut self.citizens[cid];
            c.income = income;
            c.remember(tick, format!("{} Time for a place worthy of it.", if origin == EliteOrigin::Local { "Made my first billion here." } else { "Came for a visit. Bought half a street." }));
        }
        // [ledger] A fortune made or kept out of town: in from outside.
        self.transfer(Account::Outside, Account::Person(cid), 5_000.0, Kind::Investment);
        if let Some(h) = self.citizens[cid].household.filter(|h| self.households.contains_key(*h)) {
            self.transfer(Account::Outside, Account::Household(h), 50_000.0, Kind::Investment);
        }
        // The garage: two to six supercars, the first driven every day.
        let ncars = rng.random_range(2..=6usize);
        let cars: Vec<VehicleModel> = (0..ncars).map(|k| supercar(self.citizens[cid].seed ^ k as u64)).collect();
        self.give_supercar(cid, cars[0]);
        let hid = self.citizens[cid].household;
        let mansion = self.acquire(&site, Form::Mansion, owner, hid, 2, &format!("{name} bought the whole row for a mansion. Paid us off and had us out in a day."));
        // Other homes and businesses elsewhere.
        let homes: Vec<BuildingId> = {
            let mut v: Vec<(f32, BuildingId)> = self
                .buildings
                .iter()
                .filter(|(id, b)| b.land_use == LandUse::Residential && b.tier == Tier::High && b.plot.owner.is_none() && Some(*id) != mansion && self.buyable(*id))
                .map(|(id, b)| (b.land_value, id))
                .collect();
            v.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            v.into_iter().take(rng.random_range(1..=2usize)).map(|(_, id)| id).collect()
        };
        let firms: Vec<BuildingId> = {
            let mut v: Vec<(f32, BuildingId)> = self
                .buildings
                .iter()
                .filter(|(id, b)| b.business.as_ref().is_some_and(|z| !z.closed) && b.plot.owner.is_none() && self.buyable(*id))
                .map(|(id, b)| (b.business.as_ref().map_or(0.0, |z| z.money), id))
                .collect();
            v.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            v.into_iter().take(rng.random_range(1..=3usize)).map(|(_, id)| id).collect()
        };
        for b in homes.into_iter().chain(firms) {
            let p = self.sale_price(b);
            self.give_to(b, owner, p);
        }
        let escorts = Elite::escorts_for(worth);
        let day = self.clock.day();
        self.estate.elites.push(Elite { citizen: cid, owner, net_worth: worth, origin, mansion, cars, escorts, police: false, since_day: day as u32 });
        self.refresh_police_escort();
        self.estate.stats.elites_total += 1;
        Ok(cid)
    }

    /// Only the richest, and only from `POLICE_ESCORT_WORTH`, ride behind
    /// the police.
    pub(super) fn refresh_police_escort(&mut self) {
        let top = self.estate.elites.iter().enumerate().max_by(|a, b| a.1.net_worth.total_cmp(&b.1.net_worth)).map(|(i, e)| (i, e.net_worth));
        for (i, e) in self.estate.elites.iter_mut().enumerate() {
            e.police = top.is_some_and(|(t, w)| t == i && w >= POLICE_ESCORT_WORTH);
        }
    }

    /// A visitor who stays: a new one-person household, put up in the best
    /// free home until the mansion is done. `None` without a free home.
    pub(super) fn elite_visitor(&mut self, rng: &mut SimRng) -> Option<CitizenId> {
        let home = self.buildings.iter().filter(|(_, b)| b.land_use == LandUse::Residential && b.operating() && b.free_homes() > 0).max_by(|a, b| a.1.tier.cmp(&b.1.tier).then(a.1.rent.total_cmp(&b.1.rent)).then(b.0.cmp(&a.0))).map(|(id, _)| id)?;
        let seed = self.fresh_seed();
        let family = names::family_name(rng);
        let name = names::member_name(rng, &family);
        let pos = self.building_entrance(home).unwrap_or_default();
        let tick = self.clock.tick;
        let hid = self.households.insert(Household { members: Vec::new(), home: Some(home), money: 50_000.0, groceries: 6.0, rent_stress_days: 0, moved_day: self.clock.day() });
        let look = crate::looks::family_looks(&[(seed, 48)], Tier::High).into_iter().next();
        let c = Citizen {
            thought: "Charming little city. I'll take it.".to_string(),
            mood: 0.8,
            income: 1000.0,
            savings: 5_000.0,
            satiety: 0.9,
            groceries: 6.0,
            look,
            ..Citizen::newcomer(name, seed, 48, Some(home), Some(hid), pos, tick)
        };
        let id = self.citizens.insert(c);
        self.households[hid].members.push(id);
        self.buildings[home].households.push(hid);
        self.buildings[home].residents.push(id);
        self.events.push(SimEvent::CitizenArrived(id));
        Some(id)
    }

    /// The citizen's everyday car becomes a supercar (or one is bought and
    /// parked at home).
    pub(in crate::world) fn give_supercar(&mut self, cid: CitizenId, model: VehicleModel) {
        let Some(c) = self.citizens.get(cid) else { return };
        if let Some(vid) = c.car.filter(|v| self.vehicles.cars.contains_key(*v)) {
            self.vehicles.cars[vid].model = model;
            let c = &mut self.citizens[cid];
            c.vehicle = Some(model);
            c.owns_car = true;
            return;
        }
        let near = c.home.and_then(|h| self.building_entrance(h)).unwrap_or(c.pos);
        let home = c.home;
        let Some(spot) = self.place_near(near, home) else { return };
        let vid = self.new_car(cid, model, crate::vehicle::CarPlace::Driving(CarId::default()), crate::vehicle::CarEvent::Bought);
        self.park_at(vid, spot);
    }

    /// The best quiet cluster of two to three touching residential plots:
    /// low pressure, no nuisance or fouled ground, little built round it,
    /// no industry or nightlife near; land value counts but parks hardly
    /// (the grounds are the park), and other mansions draw them.
    pub fn elite_site(&self) -> Option<Vec<LotId>> {
        self.elite_site_why().ok()
    }

    /// `elite_site`, or why there is none: how many residential plots
    /// passed each test.
    pub fn elite_site_why(&self) -> Result<Vec<LotId>, String> {
        let (mut open_n, mut quiet_n, mut clear_n) = (0usize, 0usize, 0usize);
        let loud: Vec<Vec2> = self
            .buildings
            .values()
            .filter(|b| matches!(b.land_use, LandUse::Industrial | LandUse::Entertainment) || b.plant.is_some())
            .filter_map(|b| self.lots.get(b.lot).map(|l| l.center))
            .collect();
        let mansions: Vec<Vec2> = self.buildings.iter().filter(|(_, b)| b.plot.form == Some(Form::Mansion)).filter_map(|(id, _)| self.footprint(id).map(|l| l.center)).collect();
        let open = |lid: LotId, l: &Lot| -> bool {
            // [r4-oldstone] In a historic district too, on empty plots only
            // (`buyable` keeps its standing houses).
            if l.land_use != LandUse::Residential || !self.site_clear(lid, SiteRule::ANY) {
                return false;
            }
            match l.building {
                None => true,
                Some(b) => self.buildings.get(b).is_some_and(|x| x.floors <= 2 && x.tier != Tier::High && x.plot.owner.is_none() && !x.plot.merged()) && self.buyable(b),
            }
        };
        let quiet_max = {
            let mut v: Vec<f32> = self.lots.values().filter(|l| l.building.is_some()).map(|l| self.estate.pressure.at(l.center)).collect();
            v.sort_by(|a, b| a.total_cmp(b));
            let median = v.get(v.len() / 2).copied().unwrap_or(0.0);
            ELITE_QUIET_PRESSURE.max(median * ELITE_QUIET_SHARE)
        };
        let mut cands: Vec<(f32, LotId)> = Vec::new();
        for (lid, l) in self.lots.iter() {
            if !open(lid, l) {
                continue;
            }
            open_n += 1;
            let pressure = self.estate.pressure.at(l.center);
            if pressure > quiet_max || self.land_value.nuisance_at(l.center) > ELITE_MAX_NUISANCE || self.contamination.at(l.center) > ELITE_MAX_CONTAMINATION {
                continue;
            }
            quiet_n += 1;
            if loud.iter().any(|p| p.distance(l.center) < ELITE_NUISANCE_CLEARANCE) || mansions.iter().any(|m| m.distance(l.center) < 90.0) {
                continue;
            }
            clear_n += 1;
            // Parks matter less: most of the walking-access term comes out.
            let value = self.land_value.at(l.center) - 0.8 * self.land_value.access_value_at(l.center);
            let quiet = (quiet_max - pressure) / quiet_max * 0.3;
            let company = if mansions.iter().any(|m| m.distance(l.center) < ELITE_ATTRACT_RADIUS) { 0.4 } else { 0.0 };
            cands.push((value + quiet + company, lid));
        }
        cands.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        // Partners may be any quiet plot; the best forty seed the search.
        let pool: Vec<LotId> = cands.iter().map(|c| c.1).collect();
        cands.truncate(40);
        for &(_, seed) in &cands {
            let s = &self.lots[seed];
            if s.landlocked || !s.car_access() {
                continue;
            }
            // Little built round it.
            let near_floors: u32 = self.buildings.values().filter(|b| b.land_use != LandUse::Park).filter_map(|b| self.lots.get(b.lot).map(|l| (b.floors as u32, l.center))).filter(|(_, c)| c.distance(s.center) <= ELITE_DENSITY_RADIUS).map(|(f, _)| f).sum();
            if near_floors > ELITE_MAX_NEARBY_FLOORS {
                continue;
            }
            let mut cluster = vec![seed];
            let mut area = s.area();
            for &other in &pool {
                if cluster.len() >= MAX_MERGED_LOTS || area >= ELITE_WANT_AREA {
                    break;
                }
                if cluster.contains(&other) {
                    continue;
                }
                let o = &self.lots[other];
                if !cluster.iter().any(|c| self.lots[*c].center.distance(o.center) < 100.0 && lots_touch(&self.lots[*c], o)) {
                    continue;
                }
                let others: Vec<&Lot> = cluster.iter().skip(1).chain(std::iter::once(&other)).map(|l| &self.lots[*l]).collect();
                if merged_lot(s, &others).is_some() {
                    cluster.push(other);
                    area += o.area();
                }
            }
            if cluster.len() >= 2 && area >= ELITE_MIN_AREA {
                return Ok(cluster);
            }
        }
        Err(format!("no quiet land for a mansion: {open_n} open residential plots, {quiet_n} quiet (pressure <= {quiet_max:.1}, no nuisance or fouled ground), {clear_n} clear of works and nightlife; none with a touching quiet neighbour and little built round it"))
    }

    /// NIMBY: the lift fades; while it holds, up to two cheap homes a day
    /// in the radius are bought and rebuilt dearer (never taller than
    /// `NIMBY_HEIGHT_CAP`), at most `NIMBY_PRICE_OUT_CAP` per mansion.
    pub(in crate::world) fn daily_nimby(&mut self) {
        let buildings = &self.buildings;
        self.estate.nimby.retain(|n| buildings.contains_key(n.mansion));
        for n in &mut self.estate.nimby {
            n.strength *= NIMBY_DECAY;
        }
        self.estate.nimby.retain(|n| n.strength >= 0.02);
        let mut rng = self.erng(erng_salt::NIMBY);
        for k in 0..self.estate.nimby.len() {
            let n = self.estate.nimby[k].clone();
            if n.price_out_left == 0 {
                continue;
            }
            let who = self.estate.elites.iter().find(|e| e.mansion == Some(n.mansion)).and_then(|e| self.citizens.get(e.citizen)).map(|c| c.name.clone()).unwrap_or_else(|| "a billionaire".into());
            let mut cheap: Vec<(f32, BuildingId)> = self
                .buildings
                .iter()
                .filter(|(id, b)| b.land_use == LandUse::Residential && b.tier == Tier::Low && b.plot.form.is_none() && self.buyable(*id))
                .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (l.center.distance(n.center), id)))
                .filter(|(d, _)| *d < n.radius)
                .collect();
            cheap.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let mut done = 0u8;
            for (_, bid) in cheap {
                if done >= NIMBY_PRICE_OUT_PER_DAY || self.estate.nimby[k].price_out_left == 0 {
                    break;
                }
                if !rng.random_bool(0.6) {
                    continue;
                }
                let lot = self.buildings[bid].lot;
                let value = self.lot_value(lot);
                let floors = self.buildings[bid].floors.clamp(1, NIMBY_HEIGHT_CAP);
                let memory = format!("Priced out: {who} built a mansion down the road and the landlord sold up. Rents here are for someone else now.");
                self.redevelop_with(bid, Tier::Mid, floors, value, memory, false);
                self.tag_demolition(bid, crate::buildings::DemolitionReason::PricedOut);
                self.estate.nimby[k].price_out_left -= 1;
                self.estate.stats.priced_out_total += 1;
                done += 1;
            }
        }
    }
}
